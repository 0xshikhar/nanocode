#![deny(rustdoc::broken_intra_doc_links)]

pub use nanocode_agent::Nanocodex as Agent;
pub use nanocode_agent::{
    AgentEvents, AgentSessionContext, Capabilities, ClaudeModel, CostStatus, EstimatedUsdCost,
    ExecutionPolicyDisposition, ForkPoint, ForkRequest, HarnessFamily, Lineage, ModelCapabilities,
    ModelTransport, Mutability, NanocodexError, Origin, Persistence, PromptRequest, PromptRoute,
    ReportedTurnUsage, ServiceTier, SessionCheckpoint, SessionInfo, Turn, TurnControl, TurnResult,
    TurnUsage, UsdAmount, WeakAgent,
};
pub use nanocode_agent::{
    BackendId, BackendKind, BackendRegistration as Backend, BackendRequest, ModelSelection,
};

pub use nanocode_backend_codex::NanocodexBuilder as CodexBuilder;
mod harness;
pub use harness::{Harness, HarnessBuilder, HarnessRequest};
#[cfg(not(target_family = "wasm"))]
mod runtime;
pub use nanocode_agent::{Model, ReasoningMode, Thinking};
pub use nanocode_backend_claude::{Claude, ClaudeClient};

pub use nanocode_backend_codex::{Codex, client::OpenAi};
pub use nanocode_code_mode::Tool;

pub use nanocode_code_mode::Tools;
#[cfg(not(target_family = "wasm"))]
pub use nanocode_code_mode::tool;

pub use nanocode_durability::DurableAgentExt;
#[cfg(not(target_family = "wasm"))]
pub use runtime::ClaudeHarness;
#[cfg(not(target_family = "wasm"))]
pub use runtime::CodexHarness;
#[cfg(not(target_family = "wasm"))]
pub use runtime::{Nanocodex, NanocodexBuilder, SpawnBuilder, Unselected};

pub mod agent {

    pub use crate::durability;
    pub use nanocode_agent::Nanocodex as Agent;
    pub use nanocode_agent::{
        AgentEvents, AgentSessionContext, BuilderBackend, CostStatus, EstimatedUsdCost,
        ExecutionPolicyDisposition, NanocodexError, PromptRequest, PromptRoute, ReportedTurnUsage,
        Result, ServiceTier, SpawnOptions, Turn, TurnControl, TurnResult, TurnUsage, UsdAmount,
        WeakAgent, events, input, session, usage,
    };
    pub use nanocode_agent::{
        AgentHandle, Capabilities, ForkPoint, ForkRequest, Lineage, Mutability, Origin,
        Persistence, SessionCheckpoint, SessionInfo,
    };

    pub use nanocode_backend_codex::{
        ExecutionEnvironment, NanocodexBuilder as CodexBuilder, execution,
    };
}

#[doc(inline)]
pub use nanocode_backend_claude as claude;

#[doc(inline)]
pub use nanocode_durability as durability;

#[doc(inline)]
pub use nanocode_backend_codex::client as oai;

pub use nanocode_backend_codex::client as codex;

#[doc(inline)]
pub use nanocode_code_mode as tools;

#[doc(inline)]
pub use nanocode_code_mode as oai_tools;

#[cfg(not(target_family = "wasm"))]
#[doc(inline)]
pub use nanocode_backend_claude::tools as claude_tools;

#[cfg(not(target_family = "wasm"))]
#[doc(inline)]
pub use nanocode_observability as observability;

#[cfg(not(target_family = "wasm"))]
pub use nanocode_cua as cua;
#[cfg(not(target_family = "wasm"))]
pub use nanocode_cua::{ComputerConfig, ComputerTools};

pub mod prelude {
    #[cfg(not(target_family = "wasm"))]
    pub use crate::ClaudeHarness;
    #[cfg(not(target_family = "wasm"))]
    pub use crate::CodexHarness;

    pub use crate::DurableAgentExt;
    #[cfg(not(target_family = "wasm"))]
    pub use crate::Nanocodex;
    pub use crate::Tool;

    pub use crate::Tools;
    #[cfg(not(target_family = "wasm"))]
    pub use crate::tool;
    pub use crate::{Agent, ClaudeModel, HarnessFamily, Model, ModelSelection, Thinking};
    pub use crate::{Claude, ClaudeClient};

    pub use crate::{CodexBuilder, OpenAi};
    #[cfg(not(target_family = "wasm"))]
    pub use crate::{ComputerConfig, ComputerTools};
}

#[cfg(not(target_family = "wasm"))]
#[doc(hidden)]
pub mod __private {
    pub use nanocode_code_mode::__private::*;
}

pub use nanocode_backend_codex::builtin_tools as codex_tools;

pub use nanocode_code_mode as code_mode;

pub use nanocode_tools as tool_contract;
