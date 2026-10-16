#[cfg(not(target_family = "wasm"))]
#[path = "native.rs"]
mod platform;

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
#[path = "web.rs"]
mod platform;

pub(crate) use platform::{AgentFactory, AgentSend};
pub(super) use platform::{ServiceFactory, spawn_driver};

use crate::agent::execution::ExecutionFuture;
use crate::client::{
    ResponseError,
    tower::{ResponsesAttempt, ResponsesClient, ResponsesServiceResponse},
};
use tower::Service;

pub(crate) type ResponsesClientBox = Box<dyn ResponsesExecutor>;

pub(crate) trait ResponsesExecutor: AgentSend {
    fn execute(
        &mut self,
        request: ResponsesAttempt,
    ) -> ExecutionFuture<'_, std::result::Result<ResponsesServiceResponse, ResponseError>>;
}

impl<S> ResponsesExecutor for ResponsesClient<S>
where
    S: Service<ResponsesAttempt, Response = ResponsesServiceResponse> + AgentSend + 'static,
    S::Error: Into<ResponseError> + AgentSend + 'static,
    S::Future: AgentSend,
{
    fn execute(
        &mut self,
        request: ResponsesAttempt,
    ) -> ExecutionFuture<'_, std::result::Result<ResponsesServiceResponse, ResponseError>> {
        Box::pin(async move { Self::execute(self, request).await.map_err(Into::into) })
    }
}
