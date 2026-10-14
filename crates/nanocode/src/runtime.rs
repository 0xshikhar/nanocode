use crate::claude::{ClaudeBuilder, ClaudeClient};
use crate::{
    Agent, AgentEvents, HarnessFamily, ModelSelection, NanocodexError, SessionCheckpoint, Thinking,
    harness::{AgentResult, Harness, HarnessRequest},
};

use crate::{CodexBuilder, OpenAi};
use nanocode_agent::AgentHandle;
use nanocode_agent::{
    ModelTransport, SpawnOptions, WeakAgent,
    backend::{AgentFactory, AgentOwner, BackendFuture, adopt},
};
use std::{
    future::Future,
    marker::PhantomData,
    path::{Path, PathBuf},
    pin::pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

mod computer;

type Result<T> = nanocode_agent::Result<T>;

use crate::durability::DurableBackendRegistration as RegisteredBackend;

type Recipe = Arc<dyn Fn(AgentSpec) -> BackendFuture<AgentResult> + Send + Sync>;

pub type SubagentUpdates =
    tokio::sync::mpsc::UnboundedReceiver<nanocode_subagents::ScopedAgentUpdate>;
type UpdateSlot = Arc<Mutex<Option<SubagentUpdates>>>;

type CodexHook = Arc<dyn Fn(CodexBuilder) -> Result<CodexBuilder> + Send + Sync>;
type ClaudeHook = Arc<dyn Fn(ClaudeBuilder) -> Result<ClaudeBuilder> + Send + Sync>;
type ToolsFactory = Arc<
    dyn Fn(AgentHandle) -> std::result::Result<crate::Tools, nanocode_code_mode::ToolsBuildError>
        + Send
        + Sync,
>;

#[derive(Clone)]
enum AgentTools {
    Shared(crate::Tools),
    PerAgent(ToolsFactory),
}
impl AgentTools {
    fn materialize(
        &self,
        handle: AgentHandle,
    ) -> std::result::Result<crate::Tools, nanocode_code_mode::ToolsBuildError> {
        match self {
            Self::Shared(tools) => Ok(tools.clone()),
            Self::PerAgent(factory) => factory(handle),
        }
    }
}

#[derive(Clone, Default)]
struct Settings {
    instructions: Option<Arc<str>>,
    tools: Option<AgentTools>,
}

impl Settings {
    fn or(self, fallback: &Self) -> Self {
        Self {
            instructions: self.instructions.or_else(|| fallback.instructions.clone()),
            tools: self.tools.or_else(|| fallback.tools.clone()),
        }
    }
}

macro_rules! settings_methods {
    () => {
        #[must_use]
        pub fn instructions(mut self, instructions: impl Into<Arc<str>>) -> Self {
            self.settings.instructions = Some(instructions.into());
            self
        }
    };
}

macro_rules! tool_methods {
    () => {
        #[must_use]
        pub fn tools(mut self, tools: crate::Tools) -> Self {
            self.settings.tools = Some(AgentTools::Shared(tools));
            self
        }

        #[must_use]
        pub fn tools_factory<T>(mut self, factory: T) -> Self
        where
            T: Fn(
                    AgentHandle,
                )
                    -> std::result::Result<crate::Tools, nanocode_code_mode::ToolsBuildError>
                + Send
                + Sync
                + 'static,
        {
            self.settings.tools = Some(AgentTools::PerAgent(Arc::new(factory)));
            self
        }
    };
}

#[derive(Clone)]
pub struct CodexHarness {
    client: OpenAi,
    settings: Settings,
    configure: Option<CodexHook>,
}

impl CodexHarness {
    #[must_use]
    pub fn new(client: OpenAi) -> Self {
        Self {
            client,
            settings: Settings::default(),
            configure: None,
        }
    }

    settings_methods!();
    tool_methods!();

    #[must_use]
    pub fn configure<F>(mut self, configure: F) -> Self
    where
        F: Fn(CodexBuilder) -> Result<CodexBuilder> + Send + Sync + 'static,
    {
        self.configure = Some(Arc::new(configure));
        self
    }
}

impl From<OpenAi> for CodexHarness {
    fn from(client: OpenAi) -> Self {
        Self::new(client)
    }
}

#[derive(Clone)]
pub struct ClaudeHarness {
    client: ClaudeClient,
    settings: Settings,
    configure: Option<ClaudeHook>,
}
impl ClaudeHarness {
    #[must_use]
    pub fn new(client: ClaudeClient) -> Self {
        Self {
            client,
            settings: Settings::default(),
            configure: None,
        }
    }

    settings_methods!();
    tool_methods!();

    #[must_use]
    pub fn configure<F>(mut self, configure: F) -> Self
    where
        F: Fn(ClaudeBuilder) -> Result<ClaudeBuilder> + Send + Sync + 'static,
    {
        self.configure = Some(Arc::new(configure));
        self
    }
}
impl From<ClaudeClient> for ClaudeHarness {
    fn from(client: ClaudeClient) -> Self {
        Self::new(client)
    }
}

#[derive(Clone, Default)]
pub struct Unselected;

#[derive(Clone, Default)]
pub struct NanocodexBuilder<B = Unselected> {
    selection: PhantomData<fn() -> B>,
    backends: Vec<Arc<dyn RegisteredBackend>>,

    durability:
        Option<std::result::Result<crate::durability::SessionStore, Arc<crate::durability::Error>>>,

    codex: Option<CodexHarness>,
    claude: Option<ClaudeHarness>,
    harness: Option<HarnessFamily>,
    model: Option<ModelSelection>,
    thinking: Option<Thinking>,
    settings: Settings,
    workspace: Option<PathBuf>,
    #[cfg(not(target_family = "wasm"))]
    cua: Option<crate::ComputerTools>,
    #[cfg(not(target_family = "wasm"))]
    cua_disabled: bool,
    max_subagents: Option<usize>,
    subagents_disabled: bool,
    subagent_continuation_disabled: bool,
}

impl<B> NanocodexBuilder<B> {
    #[must_use]
    pub fn backend(mut self, backend: impl RegisteredBackend + 'static) -> Self {
        self.backends.push(Arc::new(backend));
        self
    }

    #[must_use]
    pub fn durability(mut self, store: impl crate::durability::IntoSessionStore) -> Self {
        self.durability = Some(store.into_session_store().map_err(Arc::new));
        self
    }

    #[must_use]
    pub const fn max_subagents(mut self, limit: usize) -> Self {
        self.max_subagents = Some(limit);
        self.subagents_disabled = false;
        self
    }

    #[must_use]
    pub const fn without_subagents(mut self) -> Self {
        self.subagents_disabled = true;
        self
    }

    #[must_use]
    pub const fn without_subagent_continuation(mut self) -> Self {
        self.subagent_continuation_disabled = true;
        self
    }

    #[must_use]
    pub fn openai(mut self, harness: impl Into<CodexHarness>) -> Self {
        self.codex = Some(harness.into());
        self
    }

    #[must_use]
    pub fn claude(mut self, harness: impl Into<ClaudeHarness>) -> Self {
        self.claude = Some(harness.into());
        self
    }

    #[cfg(not(target_family = "wasm"))]
    #[must_use]
    pub fn cua(mut self, tools: crate::ComputerTools) -> Self {
        self.cua = Some(tools);
        self.cua_disabled = false;
        self
    }

    #[cfg(not(target_family = "wasm"))]
    #[must_use]
    pub fn without_cua(mut self) -> Self {
        self.cua = None;
        self.cua_disabled = true;
        self
    }

    #[must_use]
    pub fn thinking(mut self, thinking: Thinking) -> Self {
        self.thinking = Some(thinking);
        self
    }

    settings_methods!();

    #[must_use]
    pub fn workspace(mut self, workspace: impl Into<PathBuf>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    #[cfg(not(target_family = "wasm"))]
    fn with_default_clients(mut self) -> Self {
        let registered = |family: HarnessFamily| {
            self.backends
                .iter()
                .any(|backend| HarnessFamily::from(backend.id()) == family)
        };
        if self.codex.is_none() && !registered(HarnessFamily::Codex) {
            self.codex = Some(OpenAi::default().into());
        }
        if self.claude.is_none() && !registered(HarnessFamily::Claude) {
            self.claude = Some(ClaudeClient::default().into());
        }
        self
    }

    pub fn build(self) -> Result<Nanocodex> {
        let preferred = if self.codex.is_some() {
            Some(HarnessFamily::Codex)
        } else if self.claude.is_some() {
            Some(HarnessFamily::Claude)
        } else {
            self.backends
                .first()
                .map(|backend| HarnessFamily::from(backend.id()))
        };
        #[cfg(not(target_family = "wasm"))]
        let builder = self.with_default_clients();
        #[cfg(target_family = "wasm")]
        let builder = self;
        builder.assemble(preferred)
    }

    fn assemble(self, preferred: Option<HarnessFamily>) -> Result<Nanocodex> {
        let mut configured = Vec::new();

        if self.codex.is_some() {
            configured.push(HarnessFamily::Codex);
        }
        if self.claude.is_some() {
            configured.push(HarnessFamily::Claude);
        }
        for backend in &self.backends {
            let family: HarnessFamily = backend.id().as_str().parse().map_err(|error| {
                NanocodexError::InvalidRequest(format!("invalid backend identity: {error}"))
            })?;
            if family.as_str().trim().is_empty() {
                return Err(NanocodexError::InvalidRequest(
                    "backend identity must not be empty".into(),
                ));
            }
            backend.validate_model(&backend.default_model())?;
            if configured.contains(&family) {
                return Err(NanocodexError::InvalidRequest(format!(
                    "backend {family} is configured more than once"
                )));
            }
            configured.push(family);
        }
        let Some(fallback) = preferred
            .filter(|family| configured.contains(family))
            .or_else(|| configured.first().cloned())
        else {
            return Err(NanocodexError::InvalidRequest(
                "a Nanocodex runtime requires at least one harness backend".into(),
            ));
        };
        if let (Some(family), Some(model)) = (self.harness.as_ref(), self.model.as_ref())
            && *family != model.family()
        {
            return Err(NanocodexError::InvalidRequest(
                "model does not belong to selected harness".into(),
            ));
        }
        let family = self
            .harness
            .or_else(|| self.model.as_ref().map(ModelSelection::family))
            .unwrap_or(fallback);
        let model = self.model.unwrap_or_else(|| {
            self.backends
                .iter()
                .find(|backend| backend.id().as_str() == family.as_str())
                .map_or_else(
                    || family.default_model(),
                    |backend| ModelSelection::new(family.clone(), backend.default_model()),
                )
        });
        model.validate()?;
        if let Some(backend) = self
            .backends
            .iter()
            .find(|backend| backend.id().as_str() == family.as_str())
        {
            backend.validate_model(model.as_str())?;
        }
        let thinking = self.thinking.unwrap_or_else(|| {
            self.backends
                .iter()
                .find(|backend| {
                    backend.id().as_str() == family.as_str()
                        && backend.default_model() == model.as_str()
                })
                .and_then(|backend| backend.default_thinking())
                .unwrap_or_else(|| model.default_thinking())
        });
        model
            .capabilities(ModelTransport::Native)
            .check_thinking(thinking)?;
        if !configured.contains(&family) {
            return Err(not_configured(family));
        }
        let workspace: Arc<Path> = resolve_workspace(self.workspace)?.into();

        #[cfg(not(target_family = "wasm"))]
        let cua: computer::Cell =
            (!self.cua_disabled).then(|| Arc::new(tokio::sync::OnceCell::new_with(self.cua)));
        #[cfg(target_family = "wasm")]
        let cua: computer::Cell = None;
        let ownership = Arc::new(Ownership::default());
        let backend_defaults: Vec<_> = self
            .backends
            .iter()
            .map(|backend| {
                let model = ModelSelection::new(backend.id(), backend.default_model());
                let thinking = backend
                    .default_thinking()
                    .unwrap_or_else(|| model.default_thinking());
                (model, thinking)
            })
            .collect();
        let durability = self
            .durability
            .or_else(|| Some(default_durability()))
            .transpose()
            .map_err(|error| NanocodexError::Backend {
                backend: "durability",
                source: error,
            })?;
        let mut ephemeral = Vec::new();
        if let Some(store) = &durability {
            crate::register_builtin_formats(store).map_err(catalog_error)?;
            for backend in &self.backends {
                match backend.checkpoint_format() {
                    Some(format) => store.register_format(format).map_err(catalog_error)?,
                    None => ephemeral.push(backend.id()),
                }
            }
        }
        let (subagents, subagent_control, subagent_updates) = if self.subagents_disabled {
            (None, None, None)
        } else {
            let (registry, control, updates) = nanocode_subagents::channel(
                self.max_subagents
                    .unwrap_or(nanocode_subagents::DEFAULT_MAX_SUBAGENTS),
            );
            if !self.subagent_continuation_disabled {
                let owners = Arc::downgrade(&ownership);
                registry.set_parent_resolver(Arc::new(move |session_id: &str| {
                    owners.upgrade()?.agent(session_id)
                }));
            }
            (Some(registry), Some(control), Some(updates))
        };
        let subagent_updates: UpdateSlot = Arc::new(Mutex::new(subagent_updates));
        let mut recipes: Vec<(HarnessFamily, Recipe)> = Vec::new();

        if let Some(codex) = self.codex {
            recipes.push((
                HarnessFamily::Codex,
                codex_recipe(
                    codex,
                    &self.settings,
                    &workspace,
                    cua.clone(),
                    subagents.clone(),
                ),
            ));
        }
        if let Some(claude) = self.claude {
            recipes.push((
                HarnessFamily::Claude,
                claude_recipe(
                    claude,
                    &self.settings,
                    &workspace,
                    cua.clone(),
                    subagents.clone(),
                ),
            ));
        }
        for backend in self.backends {
            let family = backend.id().as_str().parse().map_err(|error| {
                NanocodexError::InvalidRequest(format!("invalid backend identity: {error}"))
            })?;
            recipes.push((
                family,
                registered_recipe(
                    backend,
                    &self.settings,
                    &workspace,
                    cua.clone(),
                    subagents.clone(),
                ),
            ));
        }

        if let Some(store) = &durability {
            for (family, recipe) in &mut recipes {
                if ephemeral.iter().any(|id| id.as_str() == family.as_str()) {
                    continue;
                }
                *recipe = persistent_recipe(recipe.clone(), store.clone(), workspace.clone());
            }
        }
        if subagent_control.is_some() {
            for (_, recipe) in &mut recipes {
                *recipe = unclaimed_updates_recipe(recipe.clone(), subagent_updates.clone());
            }
        }
        let mut router = Harness::builder();

        if let Some(store) = &durability {
            router = router.recovery_store(store.clone());
        }
        for (family, recipe) in &recipes {
            let recipe = Arc::clone(recipe);
            router = router.register(family.clone(), move |request: HarnessRequest| {
                recipe(AgentSpec::routed(request))
            });
        }
        for (model, thinking) in backend_defaults {
            router = router.default_settings(model, thinking);
        }
        Ok(Nanocodex {
            inner: Arc::new(Runtime {
                harness: router.build_tracked(Arc::downgrade(&ownership)),
                recipes,
                model,
                thinking,
                ownership,

                durability,
                subagents: subagent_control,
                subagent_updates,
            }),
        })
    }
}

impl NanocodexBuilder<Unselected> {
    #[must_use]
    pub fn for_backend<B: crate::BackendKind>(self) -> NanocodexBuilder<B> {
        NanocodexBuilder {
            selection: PhantomData,
            backends: self.backends,

            durability: self.durability,

            codex: self.codex,
            claude: self.claude,
            harness: Some(B::backend_id().into()),
            model: None,
            thinking: self.thinking,
            settings: self.settings,
            workspace: self.workspace,
            #[cfg(not(target_family = "wasm"))]
            cua: self.cua,
            #[cfg(not(target_family = "wasm"))]
            cua_disabled: self.cua_disabled,
            max_subagents: self.max_subagents,
            subagents_disabled: self.subagents_disabled,
            subagent_continuation_disabled: self.subagent_continuation_disabled,
        }
    }
}

impl<B: crate::BackendKind> NanocodexBuilder<B> {
    #[must_use]
    pub fn model(mut self, model: B::Model) -> Self {
        self.model = Some(ModelSelection::new(B::backend_id(), B::model_id(&model)));
        self
    }
}

fn registered_recipe(
    backend: Arc<dyn RegisteredBackend>,
    runtime: &Settings,
    workspace: &Arc<Path>,
    cua: computer::Cell,
    subagents: Option<Arc<nanocode_subagents::Registry>>,
) -> Recipe {
    let workspace = Arc::clone(workspace);
    let defaults = runtime.clone();
    Arc::new(move |spec| {
        let backend = Arc::clone(&backend);
        let cua = cua.clone();
        let workspace = Arc::clone(&workspace);
        let settings = spec.settings.or(&defaults);
        let subagents = subagents.clone();
        Box::pin(async move {
            backend.validate_model(spec.model.as_str())?;
            let cua_tools = computer::shared(computer::connect(cua).await?.as_ref());
            let mut tools = nanocode_agent::RuntimeTools::default().shared(cua_tools);
            if let Some(registry) = subagents {
                tools = tools.per_agent(Arc::new(move |handle| {
                    let Ok(SharedTools(tools)) = nanocode_subagents::install_tools(
                        SharedTools(Vec::new()),
                        handle.clone(),
                        Arc::clone(&registry),
                    );
                    tools
                }));
            }
            let request = crate::BackendRequest {
                tools,
                model: crate::ModelSelection::new(backend.id(), spec.model.as_str()),
                thinking: Some(spec.thinking),
                options: spec.options,
                workspace: workspace.to_path_buf(),
                instructions: settings
                    .instructions
                    .map(|instructions| instructions.to_string()),
                spawn_factory: spec.spawn_factory,
                host_context: spec.host_context,
                checkpoint: spec.checkpoint,
            };

            if let Some(state) = spec.durable_state {
                return backend.build_durable(request, state).await;
            }
            backend.build(request).await
        })
    })
}

fn unclaimed_updates_recipe(recipe: Recipe, updates: UpdateSlot) -> Recipe {
    Arc::new(move |spec| {
        let recipe = recipe.clone();
        let updates = updates.clone();
        Box::pin(async move {
            drop(
                updates
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take(),
            );
            recipe(spec).await
        })
    })
}

fn persistent_recipe(
    recipe: Recipe,
    store: crate::durability::SessionStore,
    workspace: Arc<Path>,
) -> Recipe {
    Arc::new(move |mut spec| {
        let recipe = recipe.clone();
        let store = store.clone();
        let workspace = workspace.clone();
        Box::pin(async move {
            if spec.durable_state.is_none()
                && let Some(checkpoint) = &spec.checkpoint
            {
                match store.load(checkpoint.session_id()).await {
                    Ok(stored) => {
                        let record = &stored.summary.record;
                        let automatic_restore = spec.parent.is_some();
                        if record.family() != checkpoint.family()
                            || (!automatic_restore && record.model != spec.model)
                            || record.lineage != *checkpoint.lineage()
                        {
                            return Err(NanocodexError::InvalidCheckpoint(
                                "checkpoint identity differs from the stored session".into(),
                            ));
                        }
                        spec.durable_state = Some(
                            store
                                .resume(checkpoint.session_id())
                                .await
                                .map_err(catalog_error)?,
                        );
                        if automatic_restore && stored.checkpoint.is_some() {
                            spec.checkpoint = None;
                        }
                    }
                    Err(crate::durability::Error::SessionNotFound { .. }) => {}
                    Err(error) => return Err(catalog_error(error)),
                }
            }
            if spec.durable_state.is_none() {
                let (id, lineage) = if let Some(checkpoint) = &spec.checkpoint {
                    (
                        checkpoint.session_id().to_owned(),
                        checkpoint.lineage().clone(),
                    )
                } else {
                    let id = uuid::Uuid::now_v7().to_string();
                    let lineage = if let Some(parent) = &spec.parent {
                        let parent_lineage = match store.load(parent.session_id()).await {
                            Ok(stored) => stored.summary.record.lineage,
                            Err(crate::durability::Error::SessionNotFound { .. }) => {
                                crate::Lineage::root(parent.session_id())
                            }
                            Err(error) => return Err(catalog_error(error)),
                        };
                        crate::Lineage::child_of(
                            &parent_lineage,
                            parent.session_id(),
                            crate::Origin::Subagent,
                        )
                    } else {
                        crate::Lineage::root(id.clone())
                    };
                    (id, lineage)
                };
                let record = crate::durability::SessionRecord::root(
                    id,
                    spec.model.clone(),
                    Some(workspace.to_path_buf()),
                )
                .with_lineage(lineage);
                spec.durable_state = Some(store.session(record).await.map_err(catalog_error)?);
            }
            recipe(spec).await
        })
    })
}

#[derive(Clone)]
pub struct Nanocodex {
    inner: Arc<Runtime>,
}

struct Runtime {
    harness: Harness,
    recipes: Vec<(HarnessFamily, Recipe)>,
    model: ModelSelection,
    thinking: Thinking,
    ownership: Arc<Ownership>,

    durability: Option<crate::durability::SessionStore>,
    subagents: Option<nanocode_subagents::SubagentControl>,
    subagent_updates: UpdateSlot,
}

impl Nanocodex {
    #[must_use]
    pub fn builder() -> NanocodexBuilder {
        NanocodexBuilder::default()
    }

    pub fn spawn(&self) -> SpawnBuilder<'_> {
        SpawnBuilder {
            selection: PhantomData,
            runtime: self,
            options: SpawnOptions::new(),
            settings: Settings::default(),
        }
    }

    #[must_use]
    pub fn harness(&self) -> HarnessFamily {
        self.inner.model.family()
    }

    #[must_use]
    pub fn model(&self) -> ModelSelection {
        self.inner.model.clone()
    }

    #[must_use]
    pub fn thinking(&self) -> Thinking {
        self.inner.thinking
    }

    #[must_use]
    pub fn is_configured(&self, family: HarnessFamily) -> bool {
        self.inner.recipe(family).is_ok()
    }

    #[must_use]
    pub fn spawn_factory(&self) -> Arc<dyn AgentFactory> {
        self.inner.harness.spawn_factory()
    }

    #[must_use]
    pub fn subagents(&self) -> Option<&nanocode_subagents::SubagentControl> {
        self.inner.subagents.as_ref()
    }

    #[must_use]
    pub fn take_subagent_updates(&self) -> Option<SubagentUpdates> {
        self.inner
            .subagent_updates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    pub async fn resume(&self, checkpoint: SessionCheckpoint) -> AgentResult {
        checkpoint.validate()?;
        let model = checkpoint.model();
        let thinking = checkpoint.thinking();
        self.construct(AgentSpec {
            parent: None,
            options: SpawnOptions::new()
                .harness_model(model.clone())
                .thinking(thinking),
            model,
            thinking,
            settings: Settings::default(),
            spawn_factory: self.spawn_factory(),
            host_context: None,
            checkpoint: Some(checkpoint),

            durable_state: None,
        })
        .await
    }

    pub async fn open_session(&self, session_id: &str) -> AgentResult {
        let store = self.inner.durability.as_ref().ok_or_else(|| {
            NanocodexError::InvalidRequest("runtime durability is not configured".into())
        })?;
        self.open(store, session_id).await
    }

    pub async fn open(
        &self,
        store: &crate::durability::SessionStore,
        session_id: &str,
    ) -> AgentResult {
        let configured = self.inner.durability.as_ref().ok_or_else(|| {
            NanocodexError::InvalidRequest("runtime durability is not configured".into())
        })?;
        if !configured.same_store(store) {
            return Err(NanocodexError::InvalidRequest(
                "session store differs from the runtime's configured durability store".into(),
            ));
        }
        let stored = configured.load(session_id).await.map_err(catalog_error)?;
        let model = stored.summary.record.model.clone();
        let thinking = stored
            .session_checkpoint()
            .map_err(catalog_error)?
            .map_or_else(
                || {
                    stored
                        .summary
                        .record
                        .initial
                        .map_or_else(|| model.default_thinking(), |initial| initial.thinking)
                },
                |checkpoint| checkpoint.thinking(),
            );
        self.inner.recipe(model.family())?;
        let inherited = stored
            .turns
            .is_empty()
            .then(|| stored.summary.record.inherited_transcript.clone())
            .filter(|transcript| !transcript.is_empty());
        let durable_state = configured.resume(session_id).await.map_err(catalog_error)?;
        let (agent, events) = self
            .construct(AgentSpec {
                parent: None,
                options: SpawnOptions::new()
                    .harness_model(model.clone())
                    .thinking(thinking),
                model,
                thinking,
                settings: Settings::default(),
                spawn_factory: self.spawn_factory(),
                host_context: None,
                checkpoint: None,
                durable_state: Some(durable_state),
            })
            .await?;
        match inherited {
            Some(transcript) => Ok((agent.with_transcript(transcript)?, events)),
            None => Ok((agent, events)),
        }
    }

    pub async fn switch(
        &self,
        agent: &Agent,
        model: ModelSelection,
        thinking: Option<Thinking>,
    ) -> AgentResult {
        model.validate()?;
        let thinking = thinking.unwrap_or_else(|| model.default_thinking());
        model
            .capabilities(ModelTransport::Native)
            .check_thinking(thinking)?;
        self.inner.recipe(model.family())?;
        if model.family() == agent.harness_family() {
            match agent.fork(crate::ForkRequest::latest()).await {
                Ok((switched, events)) => {
                    let configured = async {
                        switched.set_harness_model(model).await?;
                        switched.set_thinking(thinking).await
                    }
                    .await;
                    return match configured {
                        Ok(()) => Ok((switched, events)),
                        Err(error) => {
                            let _ = switched.shutdown().await;
                            Err(error)
                        }
                    };
                }
                Err(NanocodexError::ForkBeforeCompletedTurn) => {}
                Err(error) => return Err(error),
            }
        }
        let transcript = agent.transcript().await?;
        let durable_state = match &self.inner.durability {
            Some(store) => {
                let parent = agent.session_id();
                let (lineage, workspace) = match store.load(parent).await {
                    Ok(stored) => (
                        stored.summary.record.lineage,
                        stored.summary.record.workspace,
                    ),
                    Err(crate::durability::Error::SessionNotFound { .. }) => {
                        (crate::Lineage::root(parent), None)
                    }
                    Err(error) => return Err(catalog_error(error)),
                };
                let record = crate::durability::SessionRecord::root(
                    uuid::Uuid::now_v7().to_string(),
                    model.clone(),
                    workspace,
                )
                .with_lineage(crate::Lineage::child_of(
                    &lineage,
                    parent,
                    crate::Origin::Fork,
                ))
                .with_inherited_transcript(transcript.clone());
                Some(store.session(record).await.map_err(catalog_error)?)
            }
            None => None,
        };
        let (switched, events) = self
            .construct(AgentSpec {
                parent: None,
                options: SpawnOptions::new()
                    .harness_model(model.clone())
                    .thinking(thinking),
                model,
                thinking,
                settings: Settings::default(),
                spawn_factory: self.spawn_factory(),
                host_context: None,
                checkpoint: None,
                durable_state,
            })
            .await?;
        Ok((switched.with_transcript(transcript)?, events))
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.inner.ownership.shutdown().await
    }

    async fn construct(&self, spec: AgentSpec) -> AgentResult {
        let recipe = self.inner.recipe(spec.model.family())?;
        let ownership = Arc::clone(&self.inner.ownership);
        ownership.own(move || recipe(spec)).await
    }
}

impl Runtime {
    fn recipe(&self, family: HarnessFamily) -> Result<Recipe> {
        self.recipes
            .iter()
            .find(|(configured, _)| *configured == family)
            .map(|(_, recipe)| Arc::clone(recipe))
            .ok_or_else(|| not_configured(family))
    }
}

#[must_use = "call build().await to construct the agent"]
pub struct SpawnBuilder<'a, B = Unselected> {
    selection: PhantomData<fn() -> B>,
    runtime: &'a Nanocodex,
    options: SpawnOptions,
    settings: Settings,
}

impl<B> SpawnBuilder<'_, B> {
    pub fn thinking(mut self, thinking: Thinking) -> Self {
        self.options = self.options.thinking(thinking);
        self
    }

    pub fn instructions(mut self, instructions: impl Into<Arc<str>>) -> Self {
        self.settings.instructions = Some(instructions.into());
        self
    }

    pub async fn build(self) -> AgentResult {
        let runtime = &self.runtime.inner;
        let resolved = runtime.harness.resolve_options(
            self.options,
            runtime.model.clone(),
            runtime.thinking,
        )?;
        let model = resolved
            .selected_harness_model()
            .unwrap_or_else(|| runtime.model.clone());
        let thinking = resolved.selected_thinking().unwrap_or(runtime.thinking);
        self.runtime
            .construct(AgentSpec {
                parent: None,
                options: resolved,
                model,
                thinking,
                settings: self.settings,
                spawn_factory: self.runtime.spawn_factory(),
                host_context: None,
                checkpoint: None,

                durable_state: None,
            })
            .await
    }
}

impl<'a> SpawnBuilder<'a, Unselected> {
    pub fn backend<B: crate::BackendKind>(self) -> SpawnBuilder<'a, B> {
        SpawnBuilder {
            selection: PhantomData,
            runtime: self.runtime,
            options: self.options.harness(B::backend_id().into()),
            settings: self.settings,
        }
    }
}

impl<B: crate::BackendKind> SpawnBuilder<'_, B> {
    pub fn model(mut self, model: B::Model) -> Self {
        self.options = self
            .options
            .harness_model(ModelSelection::new(B::backend_id(), B::model_id(&model)));
        self
    }
}

struct AgentSpec {
    model: ModelSelection,
    thinking: Thinking,
    options: SpawnOptions,

    parent: Option<AgentHandle>,
    settings: Settings,
    spawn_factory: Arc<dyn AgentFactory>,
    host_context: Option<Arc<str>>,
    checkpoint: Option<SessionCheckpoint>,

    durable_state: Option<crate::durability::DurableSession>,
}

impl AgentSpec {
    fn routed(request: HarnessRequest) -> Self {
        Self {
            model: request.model,
            thinking: request.thinking,
            options: request.options,

            parent: request.parent,
            settings: Settings::default(),
            spawn_factory: request.spawn_factory,
            host_context: request.host_context,
            checkpoint: request.checkpoint,

            durable_state: request.durable_state,
        }
    }
}

fn exposed(
    tools: crate::Tools,
    cua: Option<&computer::Computer>,
) -> std::result::Result<crate::Tools, nanocode_code_mode::ToolsBuildError> {
    let builder = tools
        .into_builder()
        .exposure(nanocode_code_mode::ToolExposure::CodeModeOnly);
    computer::expose(builder, cua).build()
}

fn codex_recipe(
    harness: CodexHarness,
    runtime: &Settings,
    workspace: &Arc<Path>,
    cua: computer::Cell,
    subagents: Option<Arc<nanocode_subagents::Registry>>,
) -> Recipe {
    let CodexHarness {
        client,
        settings,
        configure,
    } = harness;
    let settings = settings.or(runtime);
    let workspace = Arc::clone(workspace);
    Arc::new(move |spec: AgentSpec| {
        let cua = cua.clone();
        let client = client.clone();
        let settings = spec.settings.or(&settings);
        let configure = configure.clone();
        let workspace = Arc::clone(&workspace);
        let subagents = subagents.clone();
        Box::pin(async move {
            let cua = computer::connect(cua).await?;
            let Some(model) = spec.model.as_codex() else {
                return Err(NanocodexError::InvalidRequest(
                    "the Codex harness requires a Codex model".into(),
                ));
            };
            let mut builder = Agent::builder(client)
                .model(model)
                .thinking(spec.thinking)
                .spawn_factory(spec.spawn_factory)
                .host_context(spec.host_context)
                .workspace(workspace.to_path_buf());
            if let Some(instructions) = settings.instructions {
                builder = builder.instructions(instructions);
            }
            if let Some(configure) = configure {
                builder = configure(builder)?.workspace(workspace.to_path_buf());
            }
            {
                let tools = settings
                    .tools
                    .unwrap_or_else(|| AgentTools::Shared(crate::Tools::default()));
                #[cfg(not(target_family = "wasm"))]
                let process_routes = spec.durable_state.as_ref().map(|state| {
                    crate::process_routes::DurableProcessRoutes::new(state.shared_store())
                });
                builder = builder.tools_factory(move |handle| {
                    let mut selected = tools.materialize(handle.clone())?;
                    #[cfg(not(target_family = "wasm"))]
                    if let Some(store) = &process_routes {
                        selected = selected.with_default_process_route_store(store.clone());
                    }
                    if let Some(registry) = &subagents {
                        selected = nanocode_subagents::install_tools(
                            selected,
                            handle,
                            Arc::clone(registry),
                        )?;
                    }
                    exposed(selected, cua.as_ref())
                });
            }
            if spec.options.stateless_http {
                builder = builder.stateless_http();
            }
            if let Some(checkpoint) = spec.checkpoint {
                builder = builder.resume(checkpoint)?;
            }

            if let Some(state) = spec.durable_state {
                use crate::DurableAgentExt as _;
                let session_id = state.state_id().parse().map_err(|error| {
                    NanocodexError::InvalidRequest(format!(
                        "invalid durable Codex session identity: {error}"
                    ))
                })?;
                builder = builder.session_id(session_id).durability(state).await?;
            }
            builder.build()
        })
    })
}
fn claude_recipe(
    harness: ClaudeHarness,
    runtime: &Settings,
    workspace: &Arc<Path>,
    cua: computer::Cell,
    subagents: Option<Arc<nanocode_subagents::Registry>>,
) -> Recipe {
    let ClaudeHarness {
        client,
        settings,
        configure,
    } = harness;
    let settings = settings.or(runtime);
    let workspace = Arc::clone(workspace);
    Arc::new(move |spec: AgentSpec| {
        let cua = cua.clone();
        let client = client.clone();
        let settings = spec.settings.or(&settings);
        let configure = configure.clone();
        let workspace = Arc::clone(&workspace);
        let subagents = subagents.clone();
        Box::pin(async move {
            let cua = computer::connect(cua).await?;
            if spec.model.family() != HarnessFamily::Claude {
                return Err(NanocodexError::InvalidRequest(
                    "the Claude harness requires a Claude model".into(),
                ));
            }
            let mut builder = Agent::builder(crate::Claude::new(client, spec.model.as_str()))
                .thinking(spec.thinking)?
                .spawn_factory(spec.spawn_factory)
                .host_context(spec.host_context)
                .workspace(workspace.to_string_lossy().into_owned());
            if let Some(instructions) = settings.instructions {
                builder = builder.system(instructions.to_string());
            }
            if let Some(configure) = configure {
                builder = configure(builder)?.workspace(workspace.to_string_lossy().into_owned());
            }
            let tools = settings.tools;
            let root = workspace.to_path_buf();
            #[cfg(not(target_family = "wasm"))]
            let process_routes = spec.durable_state.as_ref().map(|state| {
                crate::process_routes::DurableProcessRoutes::new(state.shared_store())
            });
            builder = builder.tools_factory(move |handle| {
                let selected = tools
                    .as_ref()
                    .map(|tools| tools.materialize(handle.clone()))
                    .transpose()
                    .map_err(|error| NanocodexError::InvalidRequest(error.to_string()))?;
                let mut additive = computer::shared(cua.as_ref());
                if let Some(registry) = &subagents {
                    let Ok(SharedTools(tools)) = nanocode_subagents::install_claude_tools(
                        SharedTools(Vec::new()),
                        handle.clone(),
                        Arc::clone(registry),
                    );
                    additive.extend(tools);
                }
                #[cfg(not(target_family = "wasm"))]
                {
                    nanocode_backend_claude::install_default_tools_with_process_routes(
                        root.clone(),
                        &handle,
                        selected,
                        additive,
                        process_routes.clone().map(|store| {
                            store as Arc<dyn nanocode_code_mode::attachment::ProcessRouteStore>
                        }),
                    )
                }
                #[cfg(target_family = "wasm")]
                {
                    claude_tools(root.clone(), &handle, selected, additive)
                }
            });
            #[cfg(not(target_family = "wasm"))]
            {
                builder = builder.code_only(true);
            }
            if let Some(checkpoint) = spec.checkpoint {
                builder = builder.resume(checkpoint)?;
            }

            if let Some(state) = spec.durable_state {
                use crate::DurableAgentExt as _;
                builder = builder.durability(state).await?;
            }
            builder.build()
        })
    })
}

#[cfg(target_family = "wasm")]
fn claude_tools(
    _workspace: PathBuf,
    _handle: &AgentHandle,
    selected: Option<crate::Tools>,
    additive: Vec<Arc<dyn nanocode_tools::Tool>>,
) -> Result<nanocode_backend_claude::ClaudeTools> {
    use nanocode_tools::ToolCollection as _;
    if selected.is_some() {
        return Err(NanocodexError::InvalidRequest(
            "Claude Code Mode tools are native-only; add WASM Claude tools with ClaudeHarness::configure".into(),
        ));
    }
    additive.into_iter().try_fold(
        nanocode_backend_claude::ClaudeTools::new(),
        nanocode_backend_claude::ClaudeTools::add_shared_tool,
    )
}

struct SharedTools(Vec<Arc<dyn nanocode_tools::Tool>>);

impl nanocode_tools::ToolCollection for SharedTools {
    type Error = std::convert::Infallible;

    fn add_shared_tool(
        mut self,
        tool: Arc<dyn nanocode_tools::Tool>,
    ) -> std::result::Result<Self, Self::Error> {
        self.0.push(tool);
        Ok(self)
    }
}

#[cfg(target_family = "wasm")]
fn resolve_workspace(workspace: Option<PathBuf>) -> Result<PathBuf> {
    workspace
        .filter(|workspace| workspace.is_absolute())
        .ok_or_else(|| {
            NanocodexError::InvalidRequest(
                "a WASM runtime requires an absolute workspace path".into(),
            )
        })
}

#[cfg(not(target_family = "wasm"))]
fn resolve_workspace(workspace: Option<PathBuf>) -> Result<PathBuf> {
    match workspace {
        Some(workspace) if workspace.is_absolute() => Ok(workspace),
        workspace => {
            let current = std::env::current_dir().map_err(|error| {
                NanocodexError::InvalidRequest(format!(
                    "the runtime workspace defaults to the current directory, which is unavailable: {error}"
                ))
            })?;
            Ok(workspace.map_or_else(|| current.clone(), |relative| current.join(relative)))
        }
    }
}

fn not_configured(family: HarnessFamily) -> NanocodexError {
    NanocodexError::InvalidRequest(format!(
        "{family} harness is not configured for this runtime"
    ))
}

fn catalog_error(error: crate::durability::Error) -> NanocodexError {
    match error {
        crate::durability::Error::SessionNotFound { .. } => {
            NanocodexError::InvalidRequest(error.to_string())
        }
        error => NanocodexError::Backend {
            backend: "durability",
            source: Arc::new(error),
        },
    }
}

#[derive(Default)]
pub(crate) struct Ownership {
    state: Mutex<OwnershipState>,
    idle: tokio::sync::Notify,
    stopped: tokio::sync::OnceCell<Option<Arc<ShutdownError>>>,
}

#[derive(Default)]
struct OwnershipState {
    closed: bool,
    drained: bool,
    in_flight: usize,
    owned: Vec<(usize, Agent)>,
    children: Vec<WeakAgent>,
    failures: Vec<String>,
}

struct Admission<'a>(&'a Ownership);

impl Drop for Admission<'_> {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl Ownership {
    fn lock(&self) -> MutexGuard<'_, OwnershipState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn agent(&self, session_id: &str) -> Option<Agent> {
        self.lock()
            .owned
            .iter()
            .find(|(_, agent)| agent.session_id() == session_id)
            .map(|(_, agent)| agent.clone())
    }

    fn admission(&self) -> Result<Admission<'_>> {
        self.admit()?;
        Ok(Admission(self))
    }

    async fn own<F, Fut>(self: Arc<Self>, construct: F) -> AgentResult
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = AgentResult>,
    {
        let admission = self.admission()?;
        let (agent, events) = construct().await?;
        let owner: Arc<dyn AgentOwner> = self.clone();
        let agent = adopt(agent, &owner).await?;
        drop(admission);
        Ok((agent, events))
    }

    pub(crate) async fn track_children<F>(&self, construct: F) -> Result<Vec<(Agent, AgentEvents)>>
    where
        F: Future<Output = Result<Vec<(Agent, AgentEvents)>>>,
    {
        let admission = self.admission()?;
        let created = construct.await?;
        let mut state = self.lock();
        state.children.retain(WeakAgent::is_live);
        state
            .children
            .extend(created.iter().map(|(agent, _)| agent.downgrade()));
        drop(state);
        drop(admission);
        Ok(created)
    }

    async fn shutdown(&self) -> Result<()> {
        let outcome = self
            .stopped
            .get_or_init(|| async {
                self.lock().closed = true;
                loop {
                    let mut idle = pin!(self.idle.notified());
                    idle.as_mut().enable();
                    if self.lock().in_flight == 0 {
                        break;
                    }
                    idle.await;
                }
                self.lock().drained = true;

                loop {
                    let next = self.lock().owned.last().cloned();
                    let Some((key, agent)) = next else { break };
                    let result = agent.shutdown().await;
                    let mut state = self.lock();
                    state.owned.retain(|(owned, _)| *owned != key);
                    if let Err(error) = result {
                        state.failures.push(error.to_string());
                    }
                }
                loop {
                    let next = self.lock().children.last().cloned();
                    let Some(child) = next else { break };
                    let result = child.shutdown().await;
                    let mut state = self.lock();
                    state.children.pop();
                    if let Some(Err(error)) = result {
                        state.failures.push(error.to_string());
                    }
                }
                let failures = std::mem::take(&mut self.lock().failures);
                (!failures.is_empty()).then(|| Arc::new(ShutdownError(failures)))
            })
            .await;
        match outcome {
            None => Ok(()),
            Some(error) => Err(NanocodexError::Backend {
                backend: "runtime",
                source: Arc::clone(error) as Arc<dyn std::error::Error + Send + Sync>,
            }),
        }
    }
}

impl AgentOwner for Ownership {
    fn admit(&self) -> Result<()> {
        let mut state = self.lock();
        if state.closed {
            return Err(NanocodexError::AgentStopped);
        }
        state.in_flight += 1;
        Ok(())
    }

    fn finish(&self) {
        let mut state = self.lock();
        state.in_flight -= 1;
        if state.in_flight == 0 {
            self.idle.notify_waiters();
        }
    }

    fn adopt(&self, key: usize, agent: Agent) -> bool {
        let mut state = self.lock();
        if state.drained {
            return false;
        }
        state.owned.push((key, agent));
        true
    }

    fn release(&self, key: usize) {
        self.lock().owned.retain(|(owned, _)| *owned != key);
    }
}

#[derive(Debug)]
struct ShutdownError(Vec<String>);

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} agent(s) failed to shut down: {}",
            self.0.len(),
            self.0.join("; ")
        )
    }
}

impl std::error::Error for ShutdownError {}

fn default_durability()
-> std::result::Result<crate::durability::SessionStore, Arc<crate::durability::Error>> {
    #[cfg(not(target_family = "wasm"))]
    let store = nanocode_home::AgentHome::from_env()
        .map_err(|error| crate::durability::StoreError::Backend(error.to_string()))
        .and_then(|home| {
            nanocode_store_jsonl::JsonlStore::open_home(&home.codex_home().join("nanocode"))
        })
        .map_err(crate::durability::Error::from)
        .and_then(crate::session_store);
    #[cfg(target_family = "wasm")]
    let store = crate::durability::MemoryStore::new().and_then(crate::session_store);
    store.map_err(Arc::new)
}
