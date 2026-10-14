use crate::client::responses::{ContentItem, MessageRole, ResponseHistory, ResponseItem};
use serde::{Deserialize, Serialize};

use super::ExecutionFuture;
use crate::{NanocodexError, Result};

/// A host barrier before any compaction can discard context.
///
/// Hosts must deduplicate effects by `boundary_id`, persist their result before
/// returning a receipt, and bound execution. A dropped future means cancellation.
/// Replays with a retained execution receipt do not call the host again.
pub trait BeforeCompaction: Send + Sync {
    /// Preserves useful source context, or durably records an intentional no-op.
    fn preserve(
        &self,
        request: BeforeCompactionRequest,
    ) -> ExecutionFuture<'_, Result<CompactionReceipt>>;
}

/// Bounded source text for one compaction boundary. This is data, not instructions.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeCompactionRequest {
    /// Stable effect identity; hosts must reuse their durable result on retry.
    pub boundary_id: String,
    /// Thread whose context is about to be compacted.
    pub session_id: String,
    /// Root provider-session scope, retained across child threads.
    pub root_session_id: String,
    /// At most 64 whole messages and 32 KiB of UTF-8 text, in source order.
    pub messages: Vec<CompactionMessage>,
    /// Whole older messages were omitted to stay within the bounded context budget.
    pub truncated: bool,
}

/// Source role and text; tools, developer instructions, and harness context are excluded.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CompactionMessage {
    /// Original typed message role, either `user` or `assistant`.
    pub role: MessageRole,
    /// Plain text only; images, audio, reasoning, and tool results are excluded.
    pub text: String,
}

/// Host confirmation that preservation (or its intentional no-op) is durable.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionReceipt {
    /// Nonempty host-owned durable receipt identity, at most 256 UTF-8 bytes.
    pub receipt_id: String,
}

impl CompactionReceipt {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.receipt_id.trim().is_empty() || self.receipt_id.len() > 256 {
            return Err(NanocodexError::BeforeCompactionFailed(
                "host returned an invalid durable receipt".into(),
            ));
        }
        Ok(())
    }
}

impl BeforeCompactionRequest {
    pub(crate) fn from_history(
        boundary_id: String,
        session_id: String,
        root_session_id: String,
        history: &ResponseHistory,
    ) -> Self {
        let mut messages = Vec::new();
        let mut remaining = 32 * 1024;
        let mut truncated = false;
        // Take the recent suffix without allocating an unbounded copy of history.
        'history: for item in history.iter_rev() {
            let ResponseItem::Message {
                role: role @ (MessageRole::User | MessageRole::Assistant),
                content,
                ..
            } = item
            else {
                continue;
            };
            // The harness emits these synthetic user-context frames. Never promote
            // them to user facts. Exclusion is conservative for matching user text.
            if *role == MessageRole::User && content.iter().any(|part| matches!(part,
                ContentItem::InputText { text } if text.starts_with("<environment_context>")
                    || text.starts_with("# AGENTS.md instructions") || text.starts_with("<turn_aborted>"))) { continue; }
            let mut text = String::new();
            for part in content {
                let part = match (role, part) {
                    (MessageRole::User, ContentItem::InputText { text })
                    | (MessageRole::Assistant, ContentItem::OutputText { text, .. })
                        if !text.is_empty() =>
                    {
                        text
                    }
                    _ => continue,
                };
                // Partial source can reverse the meaning (for example by dropping
                // a negation). Preserve only whole messages, including separators.
                let bytes = text
                    .len()
                    .saturating_add(part.len())
                    .saturating_add(usize::from(!text.is_empty()));
                if messages.len() == 64 || bytes > remaining {
                    truncated = true;
                    break 'history;
                }
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(part);
            }
            if !text.is_empty() {
                remaining -= text.len();
                messages.push(CompactionMessage { role: *role, text });
            }
        }
        messages.reverse();
        Self {
            boundary_id,
            session_id,
            root_session_id,
            messages,
            truncated,
        }
    }
}
