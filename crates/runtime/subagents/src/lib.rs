mod capacity;
mod diagnostics;
mod durable;
pub use durable::{
    MemorySubagentStore, RestoreReport, SubagentStore, SubagentStoreFuture, checkpoint_key,
};

pub use diagnostics::{CompletionError, CompletionErrorCode};
mod harness;
mod message;
mod model;
mod platform;
mod routing;
mod runtime;
pub use routing::{SpawnDecision, SpawnRoute, SpawnRouter};
mod task_tree;
mod tools;

pub use model::{
    AgentDescriptor, AgentId, AgentMessage, AgentMessageUpdate, AgentStatus, AgentThread,
    AgentUpdate, MessageDeliveryState, MessageDisposition, MessageId, MessagePriority,
    MessagePurpose, MessageSender, ScopedAgentUpdate, SubagentRuntimeId, ThreadId,
};
pub use runtime::{
    AgentDirectoryEntry, AgentSummary, MessageReceipt, Registry, SubagentControl, channel,
};
pub use tools::{
    AgentStartReport, AgentTask, AgentToolResult, install_tools, start_agent, start_agent_with,
    start_agents, start_agents_observed, start_fork_agent,
};

pub const DEFAULT_MAX_SUBAGENTS: usize = usize::MAX;

pub const DEFAULT_MAX_RESIDENT_SUBAGENTS: usize = 16;

pub const DURABLE_MAX_RESIDENT_SUBAGENTS: usize = 2;

pub use tools::install_claude_tools;
