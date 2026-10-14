//! Backend-independent owned agent lifecycle and implementor contracts.
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
pub use harness::{ClaudeModel, HarnessFamily, HarnessModel, ModelCapabilities, ModelTransport};
pub use nanocode_types::{Model, ReasoningMode, Thinking, events::AgentEvents};
pub use session::{
    Capabilities, ForkPoint, ForkRequest, Lineage, Mutability, Origin, Persistence,
    SessionCheckpoint, SessionInfo,
};
pub use usage::{
    CostStatus, EstimatedUsdCost, ReportedTurnUsage, ServiceTier, TurnUsage, UsdAmount,
};
/// Interface implemented by concrete backend drivers.
pub mod backend {
    pub use crate::agent::backend::*;
    pub use crate::agent::{AgentOwner, adopt, created, created_batch};
    pub use crate::session::TurnBoundary;
}
/// Shared event stream contracts.
pub mod events {
    pub use nanocode_types::events::*;
    pub use nanocode_types::responses::AgentMessageContent;
}
/// Shared prompt and attachment contracts.
pub mod input {
    pub use nanocode_types::{
        ImageDetail, Prompt, PromptInput, PromptMessage, PromptMessageRole, UserInput,
        responses::{AgentMessageContent, ContentItem},
    };
}

/// Open registration identities shared with backend plugins.
pub use nanocode_types::{BackendId, ModelSelection};

/// A backend plugin using the existing agent lifecycle and platform future boundary.
pub trait BackendRegistration: Send + Sync {
    /// Stable registration key.
    fn id(&self) -> BackendId;
    /// Default provider-native model identifier.
    fn default_model(&self) -> String;
    /// Validate a provider-native model identifier.
    fn validate_model(&self, model: &str) -> Result<()>;
    /// Construct the common lifecycle handle with backend-owned configuration.
    fn build(
        &self,
        request: BackendRequest,
    ) -> backend::BackendFuture<Result<(Nanocodex, AgentEvents)>>;
}

/// Host configuration passed to a registered backend without a durability dependency.
pub struct BackendRequest {
    /// Backend-qualified provider model.
    pub model: ModelSelection,
    /// Requested reasoning effort.
    pub thinking: Option<Thinking>,
    /// Backend-specific settings.
    pub options: SpawnOptions,
    /// Session workspace.
    pub workspace: std::path::PathBuf,
    /// Optional host instructions.
    pub instructions: Option<String>,
    /// Shared child construction policy.
    pub spawn_factory: std::sync::Arc<dyn backend::AgentFactory>,
    /// Opaque host context.
    pub host_context: Option<std::sync::Arc<str>>,
    /// Backend-owned checkpoint data.
    pub checkpoint: Option<SessionCheckpoint>,
}
