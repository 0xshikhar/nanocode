#![deny(rustdoc::broken_intra_doc_links)]

#[cfg(all(target_family = "wasm", not(target_os = "unknown")))]
compile_error!(
    "nanocode-codex supports native targets and hosted wasm*-unknown-unknown targets; WASI is not yet supported"
);

pub mod audio;

pub mod auth;

pub mod events;

mod openai;

pub mod pricing;

pub mod responses;

#[cfg(not(target_family = "wasm"))]
pub mod tls;

pub mod session;

pub mod tools;

pub mod tower;

pub mod transport;

pub(crate) use auth::{OpenAiAuth, OpenAiAuthError, OpenAiAuthMode, OpenAiAuthSnapshot};

pub(crate) use events::stream::EventSink;

pub(crate) use events::{AgentEventKind, EventError, monotonic_now_ns};

pub(crate) use openai::ModelConfig;

pub use openai::{OpenAi, OpenAiBuilder, OpenAiError};

pub(crate) use pricing::{CostStatus, EstimatedUsdCost};
pub use responses::ResponseEvent;
pub use responses::ResponseItem;

pub use responses::{
    ContentItem, FunctionOutputBody, FunctionOutputContent, MessagePhase, MessageRole,
    ResponseItemId, ToolDefinition, Usage,
};

pub use session::{
    CompletedResponse, Response, ResponseError, ResponseErrorKind, ResponseTurn, Session,
    SessionBuildError, SessionBuilder,
};

pub(crate) use tower::attempt::{
    ResponsesAttempt, ResponsesAttemptFactory, ResponsesOutput, ResponsesServiceResponse,
    TransportStats,
};

pub(crate) use tower::service::ResponsesService;

pub(crate) use tower::{
    DefaultResponsesService, ResponsesClient, ResponsesRetryPolicy, ResponsesServiceError,
};

pub(crate) use transport::EncodedRequest;

pub(crate) use transport::{
    ResponsesError, ResponsesHistory, ResponsesTransport, RetryAdvice, RetryAfter,
};

pub(crate) use tower::{attempt, middleware, service, service_error, stream};
#[cfg(not(target_family = "wasm"))]
pub(crate) use transport::connector;

pub(crate) use transport::http;

pub(crate) use transport::{socket, telemetry};

#[doc(hidden)]

pub mod __private {
    pub use crate::client::{
        events::stream::EventSink,
        openai::{
            CallerServiceFactory, LayeredServiceFactory, ModelConfig, ResponsesServiceFactory,
        },
        session::{
            context::{
                ContextManager, assign_missing_response_item_id, responses_lite_request_prefix,
            },
            state::{
                ManagedSessionState, ManagedSessionStateError, RejectedRequestRepair,
                RequestHistory,
            },
        },
        tower::attempt::ResponsesAttemptFactory,
    };

    pub mod compaction {
        pub use crate::client::session::compaction::{
            auto_compact_token_limit, install_history, trigger,
            trim_tool_outputs_to_fit_context_window,
        };
    }

    pub fn into_openai_parts<F>(openai: crate::client::OpenAi<F>) -> (ModelConfig, F)
    where
        F: ResponsesServiceFactory,
    {
        openai.into_parts()
    }

    pub fn with_code_mode_tool_names(
        profile: crate::client::responses::RequestProfile,
        names: Vec<(String, String)>,
    ) -> crate::client::responses::RequestProfile {
        profile.with_code_mode_tool_names(names)
    }
}

pub use nanocode_types::*;
