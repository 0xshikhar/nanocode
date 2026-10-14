use std::{
    any::Any,
    path::{Path, PathBuf},
    sync::Arc,
};

use nanocode::{
    Claude, ClaudeHarness, ClaudeModel, Codex, CodexHarness, Model, ModelSelection, Nanocodex,
    OpenAi, ReasoningMode, ServiceTier, Thinking, Tools,
    agent::{Agent, AgentEvents},
    claude::ClaudeClient,
    durability::SessionStore,
    tools::attachment::{AttachmentTarget, RemoteMode, RemoteRunner, RunnerHost},
};

use crate::cli::{RemoteArgs, RunnerModeArg};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Remote {
    pub tools: Tools,
    pub label: String,
    _guard: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone)]
pub struct HostConfig {
    pub workspace: PathBuf,
    pub store: Option<SessionStore>,
    pub subagents: bool,
    pub remote: Option<Remote>,
}

#[derive(Clone)]
pub struct Host {
    pub config: HostConfig,
    pub runtime: Nanocodex,
    pub pro: bool,
}

impl Host {
    pub fn build(
        config: HostConfig,
        model: &ModelSelection,
        thinking: Thinking,
        pro: bool,
    ) -> Result<Self> {
        let mut codex = CodexHarness::new(OpenAi::default());
        let mut claude = ClaudeHarness::new(ClaudeClient::default());
        if let Some(remote) = &config.remote {
            codex = codex.tools(remote.tools.clone());
            claude = claude.tools(remote.tools.clone());
        }
        if pro {
            codex = codex.configure(|builder| Ok(builder.reasoning_mode(ReasoningMode::Pro)));
        }
        let mut builder = Nanocodex::builder()
            .openai(codex)
            .claude(claude)
            .workspace(config.workspace.clone());
        builder = match &config.store {
            Some(store) => builder.durability(store.clone()),
            None => builder.durability(nanocode::durability::MemoryStore::new()?),
        };
        if !config.subagents {
            builder = builder.without_subagents();
        }
        let builder = match (model.as_codex(), model.as_claude()) {
            (Some(model), _) => builder
                .for_backend::<Codex>()
                .model(model)
                .thinking(thinking)
                .build(),
            (_, Some(model)) => builder
                .for_backend::<Claude>()
                .model(model)
                .thinking(thinking)
                .build(),
            _ => return Err(format!("unsupported model {model}").into()),
        };
        Ok(Self {
            config,
            runtime: builder?,
            pro,
        })
    }

    pub async fn start(
        &self,
        model: &ModelSelection,
        thinking: Thinking,
    ) -> Result<(Agent, AgentEvents)> {
        let started = match (model.as_codex(), model.as_claude()) {
            (Some(model), _) => {
                self.runtime
                    .spawn()
                    .backend::<Codex>()
                    .model(model)
                    .thinking(thinking)
                    .build()
                    .await
            }
            (_, Some(model)) => {
                self.runtime
                    .spawn()
                    .backend::<Claude>()
                    .model(model)
                    .thinking(thinking)
                    .build()
                    .await
            }
            _ => return Err(format!("unsupported model {model}").into()),
        };
        Ok(started?)
    }

    pub async fn open(&self, session_id: &str) -> Result<(Agent, AgentEvents)> {
        Ok(self.runtime.open_session(session_id).await?)
    }
}

pub fn parse_model(value: &str) -> Result<ModelSelection> {
    let value = value.trim();
    if let Ok(model) = value.parse::<Model>() {
        return Ok(ModelSelection::Codex(model));
    }
    if let Ok(model) = value.parse::<ClaudeModel>() {
        return Ok(ModelSelection::Claude(model));
    }
    let known = ModelSelection::for_family(nanocode::HarnessFamily::Codex)
        .chain(ModelSelection::for_family(nanocode::HarnessFamily::Claude))
        .map(|model| model.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!("unknown model {value:?}; known models: {known}").into())
}

pub fn parse_thinking(value: &str) -> Result<Thinking> {
    Thinking::ALL
        .into_iter()
        .find(|thinking| thinking.as_str().eq_ignore_ascii_case(value.trim()))
        .ok_or_else(|| {
            let known = Thinking::ALL.map(Thinking::as_str).join(", ");
            format!("unknown thinking level {value:?}; expected one of {known}").into()
        })
}

pub fn parse_tier(value: &str) -> Result<ServiceTier> {
    match value {
        "standard" => Ok(ServiceTier::Standard),
        "fast" => Ok(ServiceTier::Fast),
        "ultrafast" => Ok(ServiceTier::Ultrafast),
        other => Err(format!("unknown service tier {other:?}").into()),
    }
}

pub fn home_dir(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(home) = explicit {
        return Ok(home.to_path_buf());
    }
    let home = nanocode_home::AgentHome::from_env()?;
    Ok(home.codex_home().join("nanocode"))
}

pub fn open_store(home: &Path) -> Result<SessionStore> {
    let store = nanocode_store_jsonl::JsonlStore::open_home(home)?;
    Ok(nanocode::session_store(store)?)
}

pub async fn attach_remote(
    args: &RemoteArgs,
    workspace: &Path,
    notify: &mut dyn FnMut(&str),
) -> Result<Option<Remote>> {
    let mode = match args.runner_mode {
        RunnerModeArg::Alongside => RemoteMode::Alongside,
        RunnerModeArg::Replace => RemoteMode::Replace,
    };
    let runner_workspace = args
        .runner_workspace
        .clone()
        .unwrap_or_else(|| workspace.to_path_buf());
    let (runner, guard, label): (RemoteRunner, Arc<dyn Any + Send + Sync>, String) =
        match (&args.runner_connect, args.runner_host) {
            (Some(url), _) => {
                let token = args
                    .runner_token
                    .clone()
                    .ok_or("--runner-connect requires --runner-token or NANOCODEX_RUNNER_TOKEN")?;
                let runner = call_timeout(RemoteRunner::new(args.runner_name.clone())?, args)?;
                let connection = runner.connect(AttachmentTarget::new(url, token)?)?;
                (runner, Arc::new(connection), format!("connected to {url}"))
            }
            (None, Some(address)) => {
                let token = args
                    .runner_token
                    .clone()
                    .ok_or("--runner-host requires --runner-token or NANOCODEX_RUNNER_TOKEN")?;
                let host = RunnerHost::bind(address).await?;
                let runner = call_timeout(host.runner(args.runner_name.clone(), token)?, args)?;
                let label = format!("waiting for runner at {}", host.endpoint());
                notify(&label);
                (runner, Arc::new(host), label)
            }
            (None, None) => return Ok(None),
        };
    let remote = runner.tools().await?;
    let tools = Tools::builder()
        .remote_runner(remote, runner_workspace, mode)?
        .build()?;
    Ok(Some(Remote {
        tools,
        label: format!(
            "{} ({label}, {})",
            args.runner_name,
            match args.runner_mode {
                RunnerModeArg::Alongside => "alongside local tools",
                RunnerModeArg::Replace => "replacing local tools",
            }
        ),
        _guard: Arc::new((guard, runner)),
    }))
}

fn call_timeout(runner: RemoteRunner, args: &RemoteArgs) -> Result<RemoteRunner> {
    Ok(match args.runner_call_timeout_ms {
        Some(ms) => runner.with_call_timeout(std::time::Duration::from_millis(ms))?,
        None => runner,
    })
}
