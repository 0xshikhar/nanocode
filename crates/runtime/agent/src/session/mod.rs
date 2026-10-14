mod contract;
pub use contract::*;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptItem {
    User(String),

    Reasoning(String),

    Assistant(String),

    Tool {
        call_id: String,

        name: String,

        arguments: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_call_id: Option<String>,
    },

    ToolResult {
        call_id: String,

        output: String,

        outcome: ToolOutcome,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    Completed,

    Failed,

    Unknown,
}

impl TranscriptItem {
    pub const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

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
