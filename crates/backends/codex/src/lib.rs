#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(all(target_family = "wasm", not(target_os = "unknown")))]
compile_error!(
    "nanocode-runtime supports browser/JavaScript WebAssembly \
     (`wasm32-unknown-unknown`), not WASI targets"
);

extern crate self as nanocode_runtime;

mod agent;
mod error;
mod harness;
mod reasoning;
#[cfg(feature = "openai")]
mod registration;
#[cfg(feature = "openai")]
mod service_tier_serde;
pub use harness::{ClaudeModel, HarnessFamily, ModelCapabilities, ModelSelection, ModelTransport};
#[cfg(feature = "openai")]
mod model;
#[cfg(feature = "openai")]
mod prompt_cache;

#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub mod execution {
    pub use crate::agent::execution::*;
}

pub mod session;

pub mod usage;

#[doc(hidden)]
pub mod backend {
    pub use crate::agent::backend::*;
    pub use crate::agent::{AgentOwner, adopt};
    pub use crate::session::TurnBoundary;
}

pub use crate::client::{Model, ReasoningMode, Thinking, events::AgentEvents};
#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub use crate::client::{OpenAi, ResponseError, ResponseErrorKind};
pub use agent::{
    AgentHandle, AgentSessionContext, BuilderBackend, Nanocodex, PromptRequest, PromptRoute,
    SpawnOptions, Turn, TurnControl, TurnResult, WeakAgent,
};
#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub use agent::{ExecutionEnvironment, NanocodexBuilder};
#[cfg(feature = "openai")]
pub use error::CompactionRecovery;
pub use error::{ExecutionPolicyDisposition, NanocodexError, Result};
#[cfg(all(feature = "openai", not(target_family = "wasm")))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "openai", not(target_family = "wasm")))))]
pub use nanocode_code_mode::tool;
#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub use nanocode_code_mode::{Tool, Tools};
pub use session::{
    Capabilities, ForkPoint, ForkRequest, Lineage, Mutability, Origin, Persistence,
    SessionCheckpoint, SessionInfo,
};
pub use usage::{
    CostStatus, EstimatedUsdCost, ReportedTurnUsage, ServiceTier, TurnUsage, UsdAmount,
};

pub mod events {
    #[cfg(feature = "openai")]
    pub use crate::client::events::OpenAiEvent;
    pub use crate::client::events::{
        AcceptedInput, AgentEventData, AssistantDelta, AssistantEvent, AssistantMessage,
        CompactionCompleted, CompactionFailed, CompactionStarted, ContextEvent, EventUsage,
        ModelCallCompleted, ModelCallFailed, ModelCallStarted, ModelEvent, ModelWarmupCompleted,
        ModelWarmupFailed, ModelWarmupStarted, ReasoningEvent, ReasoningSummaryDelta, RunError,
        RunEvent, RunMetrics, RunStarted, RunStatus, RunSteered, RunTerminal, ToolCall, ToolEvent,
        ToolResultEvent, ToolStatus, TransportEvent,
    };
    pub use crate::client::events::{
        AgentEvent, AgentEventKind, AgentEventPublisher, AgentEventTiming, AgentEvents, EventError,
        TimedAgentEvent, monotonic_now_ns,
    };
    pub use crate::client::responses::AgentMessageContent;
}

pub mod input {
    pub use crate::client::{
        ImageDetail, Prompt, PromptInput, PromptMessage, PromptMessageRole, UserInput,
        responses::{AgentMessageContent, ContentItem},
    };
}

#[cfg(all(feature = "openai", not(target_family = "wasm")))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "openai", not(target_family = "wasm")))))]
pub mod transport {
    pub use crate::client::{
        responses::RequestProfile,
        tower::{
            DefaultResponsesService, ResponsesAttempt, ResponsesAttemptKind, ResponsesClient,
            ResponsesRetryPolicy, ResponsesServiceError, ResponsesServiceResponse,
        },
        transport::{ResponsesHistory, ResponsesTransport},
    };
    pub use crate::error::ResponsesError;
}

#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub mod tools {
    #[doc(inline)]
    pub use nanocode_code_mode::*;
}

#[cfg(all(feature = "openai", not(target_family = "wasm")))]
#[doc(hidden)]
pub mod __private {
    pub use nanocode_code_mode::__private::*;
}

pub mod client;

pub mod builtin_tools;
pub use error::ErrorExt;

#[cfg(feature = "openai")]
pub use agent::checkpoint::CodexCheckpointExt;

#[cfg(feature = "openai")]
pub use crate::client::OpenAi as Codex;
