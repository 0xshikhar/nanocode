//! Registration of the Codex recipe with the shared agent lifecycle.
use crate::{AgentEvents, Nanocodex, NanocodexError, OpenAi, Result};
use crate::client::{Model, ResponseError, __private::ResponsesServiceFactory};
use crate::client::tower::{ResponsesAttempt, ResponsesServiceResponse};
use crate::agent::AgentSend;
use nanocode_agent::{BackendId, BackendRegistration, BackendRequest};
use tower::Service;

impl<F> BackendRegistration for OpenAi<F>
where
    F: ResponsesServiceFactory + Send + Sync + 'static,
    F::Service: Service<ResponsesAttempt, Response = ResponsesServiceResponse> + AgentSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Error: Into<ResponseError> + AgentSend + 'static,
    <F::Service as Service<ResponsesAttempt>>::Future: AgentSend,
{
    fn id(&self) -> BackendId { BackendId::new("codex") }

    fn default_model(&self) -> String {
        crate::client::__private::into_openai_parts(self.clone()).0.model.as_str().to_owned()
    }

    fn validate_model(&self, model: &str) -> Result<()> {
        model.parse::<Model>().map(|_| ()).map_err(|error| NanocodexError::InvalidRequest(format!("{error}")))
    }

    fn build(&self, request: BackendRequest) -> nanocode_agent::backend::BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        let recipe = self.clone();
        Box::pin(async move {
            if request.model.backend != recipe.id() {
                return Err(NanocodexError::InvalidRequest("model belongs to another backend".into()));
            }
            let model = request.model.model.parse::<Model>()
                .map_err(|error| NanocodexError::InvalidRequest(format!("{error}")))?;
            let mut builder = Nanocodex::builder(recipe)
                .model(model)
                .workspace(request.workspace)
                .spawn_factory(request.spawn_factory)
                .host_context(request.host_context);
            if let Some(checkpoint) = request.checkpoint {
                builder = builder.resume(checkpoint)?;
            }
            if let Some(thinking) = request.thinking.or(request.options.thinking) {
                builder = builder.thinking(thinking);
            }
            if let Some(instructions) = request.instructions {
                builder = builder.additional_instructions(instructions);
            }
            if request.options.stateless_http {
                builder = builder.stateless_http();
            }
            builder.build()
        })
    }
}
