use std::sync::Arc;

use nanocode_tools::{ToolContext, ToolOutputBody};
use nanocode_types::responses::ResponseItem;
use serde::Deserialize;
use serde_json::{Value, value::RawValue};

pub struct OwnedToolContext {
    pub(crate) instruction_revision: Option<u64>,
    pub(crate) model: String,
    pub(crate) session_id: String,
    pub(crate) call_id: String,
    pub(crate) turn_id: Option<Arc<str>>,
    pub(crate) history: Arc<Vec<ResponseItem>>,
    pub(crate) output_token_budget: usize,
    pub(crate) host_context: Option<Arc<str>>,
}

impl OwnedToolContext {
    #[must_use]
    pub fn new(
        model: impl Into<String>,
        session_id: impl Into<String>,
        call_id: impl Into<String>,
        history: Arc<Vec<ResponseItem>>,
        output_token_budget: usize,
    ) -> Self {
        Self {
            instruction_revision: None,
            model: model.into(),
            session_id: session_id.into(),
            call_id: call_id.into(),
            turn_id: None,
            history,
            output_token_budget,
            host_context: None,
        }
    }

    #[must_use]
    pub fn from_context(context: ToolContext<'_>) -> Self {
        Self::new(
            context.model(),
            context.session_id(),
            context.call_id(),
            Arc::new(context.history().to_vec()),
            context.output_token_budget(),
        )
        .with_instruction_revision(context.instruction_revision())
        .with_host_context(context.host_context().map(Arc::from))
        .with_turn_id(context.turn_id().map(Arc::from))
    }

    #[must_use]
    pub fn as_context(&self) -> ToolContext<'_> {
        ToolContext::new(
            &self.model,
            &self.session_id,
            &self.call_id,
            self.history.as_slice(),
            self.output_token_budget,
        )
        .with_instruction_revision(self.instruction_revision)
        .with_host_context(self.host_context.as_deref())
        .with_turn_id(self.turn_id.as_deref())
    }

    #[must_use]
    pub const fn with_instruction_revision(mut self, revision: Option<u64>) -> Self {
        self.instruction_revision = revision;
        self
    }

    #[must_use]
    pub fn with_turn_id(mut self, turn_id: Option<Arc<str>>) -> Self {
        self.turn_id = turn_id;
        self
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_host_context(mut self, host_context: Option<Arc<str>>) -> Self {
        self.host_context = host_context;
        self
    }

    #[cfg(not(target_family = "wasm"))]
    pub(crate) const fn with_output_token_budget(mut self, output_token_budget: usize) -> Self {
        self.output_token_budget = output_token_budget;
        self
    }
}

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodeModeCell {
    pub origin_call_id: String,

    pub running: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeModeExecution {
    pub cell: Option<CodeModeCell>,

    pub output: ToolOutputBody,

    pub success: bool,

    #[serde(default)]
    pub nested_calls: Vec<NestedToolCall>,

    #[serde(default)]
    pub notifications: Vec<CodeModeNotification>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeModeNotification {
    pub call_id: String,

    pub text: String,
}

impl CodeModeNotification {
    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn new(call_id: &str, text: String) -> Self {
        Self {
            call_id: call_id.to_owned(),
            text,
        }
    }
}

pub enum CodeModeUpdate<'a> {
    NestedCallStarted {
        call_id: &'a str,

        name: &'a str,

        input: &'a Value,
    },

    NestedCallCompleted(&'a NestedToolCall),
}

pub trait CodeModeObserver: Send {
    fn update(&mut self, update: CodeModeUpdate<'_>);
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedToolCall {
    pub call_id: String,

    pub name: String,

    pub input: Value,

    pub output: ToolOutputBody,

    pub structured_result: Value,

    pub success: bool,

    pub started_after_ns: u64,

    pub duration_ns: u64,

    #[serde(default)]
    pub metadata: Option<Box<RawValue>>,
}
