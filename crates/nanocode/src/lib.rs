//! Alloy-style facade for the Nanocodex agents SDK.
#![deny(missing_docs, rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]

/// One owned agent lifecycle: prompts, turns, steering, settings, forks and
/// checkpoints, the same for every harness family.
///
/// Build one directly with `Agent::builder(OpenAi)` or
/// `Agent::builder(Claude)`, or construct many from a reusable
/// [`Nanocodex`] runtime.
pub use nanocode_agent::Nanocodex as Agent;
pub use nanocode_agent::{BackendId, BackendRegistration as Backend, BackendRequest, ModelSelection};
/// The single-backend Codex builder returned by `Agent::builder(openai)`.
#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub use nanocode_backend_codex::NanocodexBuilder as CodexBuilder;
pub use nanocode_agent::{
    AgentEvents, AgentSessionContext, Capabilities, ClaudeModel, CostStatus, EstimatedUsdCost,
    ExecutionPolicyDisposition, ForkPoint, ForkRequest, HarnessFamily, HarnessModel, Lineage,
    ModelCapabilities, ModelTransport, Mutability, NanocodexError, Origin, Persistence,
    PromptRequest, PromptRoute, ReportedTurnUsage, ServiceTier, SessionCheckpoint, SessionInfo,
    Turn, TurnControl, TurnResult, TurnUsage, UsdAmount, WeakAgent,
};
mod harness;
pub use harness::{Harness, HarnessBuilder, HarnessRequest};
#[cfg(not(target_family = "wasm"))]
mod runtime;
pub use nanocode_backend_claude::{Claude, ClaudeClient};
pub use nanocode_code_mode::Tool;
#[cfg(any(feature = "openai", not(target_family = "wasm")))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "openai", not(target_family = "wasm")))))]
pub use nanocode_code_mode::Tools;
#[cfg(not(target_family = "wasm"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub use nanocode_code_mode::tool;
#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub use nanocode_backend_codex::client::OpenAi;
pub use nanocode_agent::{Model, ReasoningMode, Thinking};
#[cfg(feature = "durability")]
#[cfg_attr(docsrs, doc(cfg(feature = "durability")))]
pub use nanocode_durability::DurableAgentExt;
#[cfg(not(target_family = "wasm"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub use runtime::ClaudeHarness;
#[cfg(all(not(target_family = "wasm"), feature = "openai"))]
#[cfg_attr(docsrs, doc(cfg(all(not(target_family = "wasm"), feature = "openai"))))]
pub use runtime::CodexHarness;
#[cfg(not(target_family = "wasm"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub use runtime::{Nanocodex, NanocodexBuilder, SpawnBuilder};

/// Owned agent lifecycle, builders, turns, branching, and checkpoints.
///
/// Provider and tool-runtime APIs keep their canonical detailed paths under
/// [`crate::oai`] and [`crate::tools`].
pub mod agent {
    #[cfg(feature = "durability")]
    #[cfg_attr(docsrs, doc(cfg(feature = "durability")))]
    pub use crate::durability;
    pub use nanocode_agent::Nanocodex as Agent;
    #[cfg(all(feature = "openai", not(target_family = "wasm")))]
    #[cfg_attr(docsrs, doc(cfg(all(feature = "openai", not(target_family = "wasm")))))]
    pub use nanocode_backend_codex::rollout;
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
    #[cfg(feature = "openai")]
    #[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
    pub use nanocode_backend_codex::{ExecutionEnvironment, NanocodexBuilder as CodexBuilder, execution};
}

/// Anthropic Messages client, provider-native builder, protocol, and authentication.
#[doc(inline)]
pub use nanocode_backend_claude as claude;

/// Portable durable execution policy and host-store contracts.
#[cfg(feature = "durability")]
#[cfg_attr(docsrs, doc(cfg(feature = "durability")))]
#[doc(inline)]
pub use nanocode_durability as durability;

/// Tower-native OpenAI Responses client, sessions, protocol, and transport.
#[doc(inline)]
pub use nanocode_backend_codex::client as oai;

/// Codex Responses client, authentication, protocol and transport.
pub use nanocode_backend_codex::client as codex;

/// OpenAI Responses tool registry, built-ins, MCP, tool search, and Code Mode.
///
/// Compatibility facade for [`crate::oai_tools`]; Claude adapters live under
/// `nanocode::claude_tools` on native targets.
#[doc(inline)]
pub use nanocode_code_mode as tools;

/// OpenAI-specific tool contracts, catalog and execution runtime.
#[doc(inline)]
pub use nanocode_code_mode as oai_tools;

/// Claude-native tool adapters and host capability contracts.
#[cfg(not(target_family = "wasm"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
#[doc(inline)]
pub use nanocode_claude_tools as claude_tools;

/// Application-owned tracing and OpenTelemetry setup.
#[cfg(all(not(target_family = "wasm"), feature = "observability"))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(not(target_family = "wasm"), feature = "observability")))
)]
#[doc(inline)]
pub use nanocode_observability as observability;

/// Thin transport for the official upstream Sky CUA/Node REPL provider.
#[cfg(not(target_family = "wasm"))]
pub use nanocode_cua as cua;
#[cfg(not(target_family = "wasm"))]
pub use nanocode_cua::{ComputerConfig, ComputerTools};

/// Common imports for the golden owned-agent path.
pub mod prelude {
    #[cfg(not(target_family = "wasm"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
    pub use crate::ClaudeHarness;
    #[cfg(all(not(target_family = "wasm"), feature = "openai"))]
    #[cfg_attr(docsrs, doc(cfg(all(not(target_family = "wasm"), feature = "openai"))))]
    pub use crate::CodexHarness;
    #[cfg(feature = "durability")]
    #[cfg_attr(docsrs, doc(cfg(feature = "durability")))]
    pub use crate::DurableAgentExt;
    #[cfg(not(target_family = "wasm"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
    pub use crate::Nanocodex;
    pub use crate::Tool;
    #[cfg(any(feature = "openai", not(target_family = "wasm")))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "openai", not(target_family = "wasm")))))]
    pub use crate::Tools;
    #[cfg(not(target_family = "wasm"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
    pub use crate::tool;
    pub use crate::{Agent, ClaudeModel, HarnessFamily, HarnessModel, Model, Thinking};
    pub use crate::{Claude, ClaudeClient};
    #[cfg(feature = "openai")]
    #[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
    pub use crate::{CodexBuilder, OpenAi};
    #[cfg(not(target_family = "wasm"))]
    pub use crate::{ComputerConfig, ComputerTools};
}

#[cfg(not(target_family = "wasm"))]
#[doc(hidden)]
pub mod __private {
    pub use nanocode_code_mode::__private::*;
}

/// Shared Code Mode and tool execution runtime.
pub use nanocode_code_mode as code_mode;
/// Codex-specific web and image tools.
pub use nanocode_backend_codex::builtin_tools as codex_tools;
/// Shared tool contracts and typed macros for all harnesses.
pub use nanocode_tools as tool_contract;
