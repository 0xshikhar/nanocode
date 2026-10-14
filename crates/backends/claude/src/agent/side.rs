use super::{
    Cancellation, ContentBlock, CostStatus, Message, NanocodexError, ReportedTurnUsage,
    ResponseContext, Result, Role, State, TurnUsage, provider_error,
};
use nanocode_agent::{SideAnswer, backend::side_question_prompt};
use std::sync::Arc;

const RUNNING_TOOL_RESULT: &str =
    "This tool call is still running in the main conversation; its result is not available yet.";

const PENDING_REQUEST: &str = "<system-reminder>The user message above is still being handled by the main agent. Do not answer it or act on it here; only answer the side question below.</system-reminder>";

fn side_messages(mut messages: Vec<Message>, question: &str) -> Vec<Message> {
    let running = messages
        .last()
        .filter(|message| message.role == Role::Assistant)
        .map(|message| {
            message
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, .. } => {
                        Some(ContentBlock::tool_result(id, RUNNING_TOOL_RESULT, false))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let text = ContentBlock::text(side_question_prompt(question));
    match messages.last_mut() {
        Some(message) if message.role == Role::User => {
            if !message
                .content
                .iter()
                .all(|block| matches!(block, ContentBlock::ToolResult { .. }))
            {
                message.content.push(ContentBlock::text(PENDING_REQUEST));
            }
            message.content.push(text);
        }
        _ => messages.push(Message {
            role: Role::User,
            content: running.into_iter().chain([text]).collect(),
        }),
    }
    messages
}

impl State {
    pub(super) fn publish_side_view(&self, messages: Option<Vec<Message>>) {
        *self.side_view.write().expect("side view lock poisoned") = messages.map(Arc::new);
    }

    async fn side_transcript(&self) -> Result<Vec<Message>> {
        let live = self.conversation.try_lock().is_err().then(|| {
            self.side_view
                .read()
                .expect("side view lock poisoned")
                .clone()
        });
        if let Some(Some(messages)) = live {
            return Ok(messages.as_ref().clone());
        }
        Ok(self.latest_boundary().await?.conversation.packed_messages())
    }
}

impl State {
    pub(super) async fn side_question(&self, question: String) -> Result<SideAnswer> {
        if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(NanocodexError::AgentStopped);
        }
        let messages = side_messages(self.side_transcript().await?, &question);
        let template = self.request_template(self.speed());
        let wire_profile = self
            .client
            .freeze_wire_profile(template.cache_control.is_some());
        let response = self
            .response(
                &messages,
                template.tools.clone(),
                &Cancellation::default(),
                None,
                0,
                ResponseContext {
                    disable_tools: false,
                    container: None,
                    previous_message_id: None,
                    template: Some(&template),
                    wire_profile: Some(&wire_profile),
                    effect: None,
                },
            )
            .await
            .map_err(|failure| failure.error)?
            .message;
        let text = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        if text.trim().is_empty() {
            return Err(provider_error(
                if response
                    .content
                    .iter()
                    .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
                {
                    "the side question response called a tool instead of answering directly"
                } else {
                    "the side question response contained no text"
                },
            ));
        }
        let usage = &response.usage;
        Ok(SideAnswer::new(
            text,
            Some(TurnUsage::from_reported(ReportedTurnUsage {
                input_tokens: usage.input_tokens,
                cached_input_tokens: usage.cache_read_input_tokens,
                cache_write_input_tokens: usage.cache_creation_input_tokens,
                output_tokens: usage.output_tokens,
                reasoning_output_tokens: 0,
                total_tokens: usage
                    .input_tokens
                    .saturating_add(usage.cache_read_input_tokens)
                    .saturating_add(usage.cache_creation_input_tokens)
                    .saturating_add(usage.output_tokens),
                estimated_cost: None,
                cost_status: CostStatus::Other,
            })),
        ))
    }
}
