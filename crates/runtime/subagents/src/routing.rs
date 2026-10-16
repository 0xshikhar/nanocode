use async_trait::async_trait;
use nanocode_agent_core::SpawnOptions;

pub struct SpawnRoute {
    pub options: SpawnOptions,
    pub reference: String,
}

impl SpawnRoute {
    pub(crate) fn validate(&self, requested: SpawnOptions) -> std::io::Result<()> {
        let model = self
            .options
            .selected_harness_model()
            .ok_or_else(|| std::io::Error::other("subagent route must select a model"))?;
        let thinking = self
            .options
            .selected_thinking()
            .ok_or_else(|| std::io::Error::other("subagent route must select thinking"))?;
        self.options
            .validate_harness()
            .map_err(std::io::Error::other)?;
        if requested
            .selected_harness()
            .is_some_and(|family| family != model.family())
        {
            return Err(std::io::Error::other(
                "subagent route conflicts with explicit harness",
            ));
        }
        if let (Some(requested), Some(selected)) = (
            requested.selected_reasoning_mode(),
            self.options.selected_reasoning_mode(),
        ) && requested != selected
        {
            return Err(std::io::Error::other(
                "subagent route conflicts with explicit reasoning mode",
            ));
        }
        if requested
            .selected_harness_model()
            .is_some_and(|value| value != model)
            || requested
                .selected_thinking()
                .is_some_and(|value| value != thinking)
        {
            return Err(std::io::Error::other(
                "subagent route conflicts with explicit override",
            ));
        }
        if !model.supports_thinking(thinking) {
            return Err(std::io::Error::other(
                "subagent route selected unsupported thinking",
            ));
        }
        if self.reference.trim().is_empty() {
            return Err(std::io::Error::other("empty subagent route reference"));
        }
        Ok(())
    }
}

pub enum SpawnDecision {
    Routed(SpawnRoute),
    Native { reference: String },
}

impl SpawnDecision {
    pub(crate) fn validate(&self, requested: SpawnOptions) -> std::io::Result<()> {
        match self {
            Self::Routed(route) => route.validate(requested),
            Self::Native { reference } => {
                if reference.trim().is_empty() {
                    return Err(std::io::Error::other("invalid native subagent choice"));
                }
                Ok(())
            }
        }
    }

    pub(crate) fn options(&self, requested: SpawnOptions) -> SpawnOptions {
        match self {
            Self::Routed(route) => {
                let mut options = route.options.clone();
                if options.selected_reasoning_mode().is_none() {
                    options.reasoning_mode = requested.selected_reasoning_mode();
                }
                options
            }
            Self::Native { .. } => requested,
        }
    }

    pub(crate) fn reference(&self) -> &str {
        match self {
            Self::Routed(route) => &route.reference,
            Self::Native { reference } => reference,
        }
    }
}

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait SpawnRouter: Send + Sync {
    async fn resolve(
        &self,
        parent_session_id: &str,
        role: &str,
        task: &str,
        options: SpawnOptions,
        host_context: Option<&str>,
    ) -> std::io::Result<SpawnRoute>;

    async fn resolve_spawn(
        &self,
        parent_session_id: &str,
        role: &str,
        task: &str,
        options: SpawnOptions,
        host_context: Option<&str>,
    ) -> std::io::Result<SpawnDecision> {
        self.resolve(parent_session_id, role, task, options, host_context)
            .await
            .map(SpawnDecision::Routed)
    }

    fn bind(
        &self,
        parent_session_id: &str,
        child_session_id: &str,
        reference: &str,
        host_context: Option<&str>,
    ) -> std::io::Result<()>;
}
