//! Tower-native OpenAI Responses API and managed context for Nanocodex.
#![cfg_attr(not(feature = "client"), doc = include_str!("../../CONTRACTS.md"))]
#![deny(missing_docs, rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(all(target_family = "wasm", not(target_os = "unknown")))]
compile_error!(
    "nanocode-codex supports native targets and hosted wasm*-unknown-unknown targets; WASI is not yet supported"
);

/// Duration-based audio token estimates shared by context and tool budgets.
#[cfg(feature = "client")]
pub mod audio;
/// Authentication sources and managed credential snapshots.
#[cfg(feature = "client")]
pub mod auth;
/// Complete typed lifecycle events emitted around Responses operations.
#[cfg(feature = "events")]
pub mod events;
#[cfg(feature = "client")]
mod openai;
/// Automatic model-specific USD estimates from provider token usage.
pub mod pricing;
/// Bidirectional GPT Realtime audio sessions and typed conversation events.
#[cfg(all(feature = "realtime", not(target_family = "wasm")))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(feature = "realtime", not(target_family = "wasm"))))
)]
pub mod realtime;
/// Complete typed request, event, and item model for the Responses protocol.
pub mod responses;
/// Shared, bounded caching of the native system trust store for TLS clients.
#[cfg(all(feature = "native-tls", not(target_family = "wasm")))]
pub mod tls;

/// Managed session identities, inputs, and compaction results.
#[cfg(feature = "client")]
pub mod session;
/// Tool contracts shared by agent loops and concrete tool runtimes.
pub mod tools;
/// Generic Tower attempt, service, retry, and streamed-output contracts.
#[cfg(feature = "client")]
pub mod tower;
/// Responses transport policy, errors, and connection statistics.
#[cfg(feature = "client")]
pub mod transport;

#[cfg(feature = "client")]
pub(crate) use auth::{OpenAiAuth, OpenAiAuthError, OpenAiAuthMode, OpenAiAuthSnapshot};
#[cfg(feature = "client")]
pub(crate) use events::stream::EventSink;
#[cfg(feature = "client")]
pub(crate) use events::{AgentEventKind, EventError, monotonic_now_ns};
#[cfg(feature = "client")]
pub(crate) use openai::ModelConfig;
#[cfg(feature = "client")]
pub use openai::{OpenAi, OpenAiBuilder, OpenAiError};
#[cfg(feature = "client")]
pub(crate) use pricing::{CostStatus, EstimatedUsdCost};
pub use responses::ResponseEvent;
pub use responses::ResponseItem;
#[cfg(feature = "client")]
pub use responses::{
    ContentItem, FunctionOutputBody, FunctionOutputContent, MessagePhase, MessageRole,
    ResponseItemId, ToolDefinition, Usage,
};
#[cfg(feature = "client")]
pub use session::{
    CompletedResponse, Response, ResponseError, ResponseErrorKind, ResponseTurn, Session,
    SessionBuildError, SessionBuilder,
};
#[cfg(feature = "client")]
pub(crate) use tower::attempt::{
    ResponsesAttempt, ResponsesAttemptFactory, ResponsesOutput, ResponsesServiceResponse,
    TransportStats,
};
#[cfg(feature = "client")]
pub(crate) use tower::service::ResponsesService;
#[cfg(feature = "client")]
pub(crate) use tower::{
    DefaultResponsesService, ResponsesClient, ResponsesRetryPolicy, ResponsesServiceError,
};
#[cfg(feature = "client")]
pub(crate) use transport::EncodedRequest;
#[cfg(feature = "client")]
pub(crate) use transport::{
    ResponsesError, ResponsesHistory, ResponsesTransport, RetryAdvice, RetryAfter,
};

#[cfg(feature = "client")]
pub(crate) use tower::{attempt, middleware, service, service_error, stream};
#[cfg(all(feature = "client", not(target_family = "wasm")))]
pub(crate) use transport::connector;
#[cfg(feature = "client")]
pub(crate) use transport::http;
#[cfg(feature = "client")]
pub(crate) use transport::{socket, telemetry};

/// Internal bridge for the higher-level `nanocode-runtime` crate.
///
/// This namespace is not a supported caller API. It keeps mutable model
/// configuration, event emission authority, and attempt construction out of
/// the normal rustdoc surface while allowing the separately versioned agent
/// crate to compose this crate without duplicating those mechanics.
#[doc(hidden)]
#[cfg(feature = "client")]
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

    /// Agent-owned context accounting and compaction policy primitives.
    pub mod compaction {
        pub use crate::client::session::compaction::{
            auto_compact_token_limit, install_history, trigger,
            trim_tool_outputs_to_fit_context_window,
        };
    }

    /// Decomposes a validated client recipe for the higher-level agent driver.
    pub fn into_openai_parts<F>(openai: crate::client::OpenAi<F>) -> (ModelConfig, F)
    where
        F: ResponsesServiceFactory,
    {
        openai.into_parts()
    }

    /// Installs the server-visible mapping for tools nested under Code Mode.
    pub fn with_code_mode_tool_names(
        profile: crate::client::responses::RequestProfile,
        names: Vec<(String, String)>,
    ) -> crate::client::responses::RequestProfile {
        profile.with_code_mode_tool_names(names)
    }
}

/// Shared SDK input and model contracts.
pub use nanocode_types::*;
