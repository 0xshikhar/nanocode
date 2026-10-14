use nanocode_agent::{
    AgentEvents, BackendRegistration, BackendRequest, Nanocodex, Result, backend::BackendFuture,
};

use crate::DurableSession;

pub trait DurableBackendRegistration: BackendRegistration {
    fn build_durable(
        &self,
        request: BackendRequest,
        state: DurableSession,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>>;
}

#[cfg(not(target_family = "wasm"))]
trait PlatformSend: Send {}
#[cfg(not(target_family = "wasm"))]
impl<T: Send> PlatformSend for T {}
#[cfg(target_family = "wasm")]
trait PlatformSend {}
#[cfg(target_family = "wasm")]
impl<T> PlatformSend for T {}

use crate::DurableAgentExt as _;
use nanocode_backend_codex::{
    NanocodexBuilder, OpenAi,
    client::{
        __private::ResponsesServiceFactory,
        ResponseError,
        tower::{ResponsesAttempt, ResponsesServiceResponse},
    },
};
use tower::Service;

impl<F> DurableBackendRegistration for NanocodexBuilder<F>
where
    F: ResponsesServiceFactory + Send + Sync + 'static,
    F::Service:
        Service<ResponsesAttempt, Response = ResponsesServiceResponse> + PlatformSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Error: Into<ResponseError> + PlatformSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Future: PlatformSend,
{
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
                .parse::<nanocode_backend_codex::session::SessionId>()
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
    fn build_durable(
        &self,
        request: BackendRequest,
        state: DurableSession,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        Nanocodex::builder(self.clone()).build_durable(request, state)
    }
}

impl DurableBackendRegistration for nanocode_backend_claude::ClaudeBuilder {
    fn build_durable(
        &self,
        request: BackendRequest,
        state: DurableSession,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        let prepared = self.clone().prepare(request);
        Box::pin(async move { prepared?.durability(state).await?.build() })
    }
}

impl DurableBackendRegistration for nanocode_backend_claude::Claude {
    fn build_durable(
        &self,
        request: BackendRequest,
        state: DurableSession,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        Nanocodex::builder(self.clone()).build_durable(request, state)
    }
}
