use std::sync::Arc;

use nanocode_agent::{AgentEvents, BackendRequest, Nanocodex, Result, backend::BackendFuture};
use nanocode_durability::{
    CheckpointFormat, DurableAgentExt as _, DurableBackendRegistration, DurableSession,
};
use tower::Service;

use crate::{
    NanocodexBuilder, OpenAi,
    client::{
        __private::ResponsesServiceFactory,
        ResponseError,
        tower::{ResponsesAttempt, ResponsesServiceResponse},
    },
    durability::CodexCheckpointFormat,
};

#[cfg(not(target_family = "wasm"))]
trait PlatformSend: Send {}
#[cfg(not(target_family = "wasm"))]
impl<T: Send> PlatformSend for T {}
#[cfg(target_family = "wasm")]
trait PlatformSend {}
#[cfg(target_family = "wasm")]
impl<T> PlatformSend for T {}

impl<F> DurableBackendRegistration for NanocodexBuilder<F>
where
    F: ResponsesServiceFactory + Send + Sync + 'static,
    F::Service:
        Service<ResponsesAttempt, Response = ResponsesServiceResponse> + PlatformSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Error: Into<ResponseError> + PlatformSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Future: PlatformSend,
{
    fn checkpoint_format(&self) -> Arc<dyn CheckpointFormat> {
        Arc::new(CodexCheckpointFormat)
    }

    fn build_durable(
        &self,
        request: BackendRequest,
        state: DurableSession,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        let prepared = self.clone().prepare(request);
        Box::pin(async move {
            let builder = prepared?;
            let session_id = state
                .state_id()
                .parse::<crate::session::SessionId>()
                .map_err(|error| {
                    nanocode_agent::NanocodexError::InvalidRequest(format!(
                        "registered durable Codex state ID must be a UUID: {error}"
                    ))
                })?;
            builder
                .session_id(session_id)
                .durability(state)
                .await?
                .build()
        })
    }
}

impl<F> DurableBackendRegistration for OpenAi<F>
where
    F: ResponsesServiceFactory + Send + Sync + 'static,
    F::Service:
        Service<ResponsesAttempt, Response = ResponsesServiceResponse> + PlatformSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Error: Into<ResponseError> + PlatformSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Future: PlatformSend,
{
    fn checkpoint_format(&self) -> Arc<dyn CheckpointFormat> {
        Arc::new(CodexCheckpointFormat)
    }

    fn build_durable(
        &self,
        request: BackendRequest,
        state: DurableSession,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        Nanocodex::builder(self.clone()).build_durable(request, state)
    }
}
