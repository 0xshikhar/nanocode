//! Typed persistence bridge for independently registered backend plugins.

use nanocode_agent::{
    AgentEvents, BackendRegistration, BackendRequest, Nanocodex, Result,
    backend::BackendFuture,
};

use crate::DurableSession;

/// A backend registration that can attach a durable owner before construction.
///
/// Implementations prepare their ordinary typed builder, attach the supplied
/// state with [`crate::DurableAgentExt`], then build the common lifecycle handle.
/// The persistence dependency stays here rather than in the agent core.
pub trait DurableBackendRegistration: BackendRegistration {
    /// Builds an agent owning the supplied durable state, restoring its latest
    /// checkpoint and retaining its execution policy for subsequent turns.
    fn build_durable(
        &self,
        request: BackendRequest,
        state: DurableSession,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>>;
}
