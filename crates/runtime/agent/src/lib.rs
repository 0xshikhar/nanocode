mod agent;
mod error;
mod harness;
pub mod session;
pub mod usage;
pub use agent::{
    AgentHandle, AgentSessionContext, BuilderBackend, Nanocodex, PromptRequest, PromptRoute,
    SpawnOptions, Turn, TurnControl, TurnResult, WeakAgent,
};
pub use error::{
    BackendError, BackendFailure, CompactionRecovery, ExecutionPolicyDisposition, NanocodexError,
    Result,
};
pub use harness::{ClaudeModel, HarnessFamily, ModelCapabilities, ModelSelection, ModelTransport};
pub use nanocode_types::{Model, ReasoningMode, Thinking, events::AgentEvents};
pub use session::{
    Capabilities, ForkPoint, ForkRequest, Lineage, Mutability, Origin, Persistence,
    SessionCheckpoint, SessionInfo,
};
pub use usage::{
    CostStatus, EstimatedUsdCost, ReportedTurnUsage, ServiceTier, TurnUsage, UsdAmount,
};

pub mod backend {
    pub use crate::agent::backend::*;
    pub use crate::agent::{AgentOwner, adopt, created, created_batch};
    pub use crate::session::TurnBoundary;
}

pub mod events {
    pub use nanocode_types::events::*;
    pub use nanocode_types::responses::AgentMessageContent;
}

pub mod input {
    pub use nanocode_types::{
        ImageDetail, Prompt, PromptInput, PromptMessage, PromptMessageRole, UserInput,
        responses::{AgentMessageContent, ContentItem},
    };
}

pub use nanocode_types::BackendId;

pub trait BackendKind {
    type Model: Clone + Send + Sync + 'static;

    fn backend_id() -> BackendId;

    fn model_id(model: &Self::Model) -> &str;
}

pub trait BackendRegistration: Send + Sync {
    fn id(&self) -> BackendId;

    fn default_model(&self) -> String;

    fn default_thinking(&self) -> Option<Thinking> {
        None
    }

    fn validate_model(&self, model: &str) -> Result<()>;

    fn build(
        &self,
        request: BackendRequest,
    ) -> backend::BackendFuture<Result<(Nanocodex, AgentEvents)>>;
}

pub struct BackendRequest {
    pub model: ModelSelection,

    pub thinking: Option<Thinking>,

    pub options: SpawnOptions,

    pub workspace: std::path::PathBuf,

    pub instructions: Option<String>,

    pub spawn_factory: std::sync::Arc<dyn backend::AgentFactory>,

    pub host_context: Option<std::sync::Arc<str>>,

    pub additive_tools: Vec<std::sync::Arc<dyn nanocode_tools::Tool>>,

    pub checkpoint: Option<SessionCheckpoint>,
}
