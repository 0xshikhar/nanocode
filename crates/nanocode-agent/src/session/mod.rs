//! Session identity and opaque backend checkpoints.
mod contract;
pub use contract::*;
/// User-visible activity of a stored session, shared by every harness and
/// store (Codex-compatible rollouts and durable catalogs).
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptItem {
    /// A submitted user prompt.
    User(String),
    /// A reasoning summary displayed while the assistant was working.
    Reasoning(String),
    /// An assistant message displayed by the originating client.
    Assistant(String),
    /// A tool invocation displayed by the originating client.
    Tool {
        /// Stable call identifier.
        call_id: String,
        /// Tool name sent by the model.
        name: String,
        /// Serialized tool arguments sent by the model.
        arguments: String,
        /// Code Mode cell that issued this call, when it is a nested call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_call_id: Option<String>,
    },
    /// The settled outcome of an earlier [`Self::Tool`] call.
    ToolResult {
        /// Call identifier of the matching [`Self::Tool`].
        call_id: String,
        /// Bounded model-visible text; media is represented by placeholders.
        output: String,
        /// Outcome as recorded; unknown is never presented as failure.
        outcome: ToolOutcome,
    },
}

/// Recorded outcome of a replayed tool call.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    /// The tool returned without reporting an error.
    Completed,
    /// The tool reported an error.
    Failed,
    /// No outcome was recorded, or the recorded outcome is unknown.
    Unknown,
}

impl TranscriptItem {
    /// Maximum retained bytes of one replayed tool outcome.
    pub const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

    /// Creates a tool outcome bounded to [`Self::MAX_TOOL_OUTPUT_BYTES`].
    #[must_use]
    pub fn tool_result(call_id: impl Into<String>, output: &str, outcome: ToolOutcome) -> Self {
        let mut end = output.len().min(Self::MAX_TOOL_OUTPUT_BYTES);
        while !output.is_char_boundary(end) {
            end -= 1;
        }
        let mut bounded = output[..end].to_owned();
        if end < output.len() {
            bounded.push_str("\n[output truncated]");
        }
        Self::ToolResult {
            call_id: call_id.into(),
            output: bounded,
            outcome,
        }
    }
}

#[cfg(not(target_family = "wasm"))]
mod rollout;
#[cfg(not(target_family = "wasm"))]
pub use rollout::RolloutInfo;
