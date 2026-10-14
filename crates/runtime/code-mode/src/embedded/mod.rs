mod input;
mod runtime;
mod types;

use std::{error::Error, fmt, future::Future, pin::Pin};

use crate::{ToolContext, ToolDefinition, ToolInput, ToolOutput};

pub use input::{prepare_output_images, prepare_user_input};
pub use runtime::{EmbeddedToolRuntime, EmbeddedToolRuntimeControl};
pub use types::{
    CodeModeCell, CodeModeExecution, CodeModeNotification, CodeModeObserver, CodeModeUpdate,
    NestedToolCall, OwnedToolContext,
};

#[doc(hidden)]
#[must_use]
pub fn bind_host(mut tools: crate::Tools, host: impl CodeModeHost) -> crate::Tools {
    tools.embedded_host = Some(std::sync::Arc::new(host));
    tools
}

#[cfg(not(target_family = "wasm"))]
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(target_family = "wasm")]
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[derive(Debug)]
pub struct CodeModeHostError {
    message: Box<str>,
}

impl CodeModeHostError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into().into_boxed_str(),
        }
    }

    #[must_use]
    pub const fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CodeModeHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CodeModeHostError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EmbeddedToolMode {
    #[default]
    Code,

    CodeOnly,

    Direct,
}

pub trait CodeModeHost: Send + Sync + 'static {
    fn tool_mode(&self) -> EmbeddedToolMode {
        EmbeddedToolMode::Code
    }

    fn supports_cells(&self) -> bool {
        false
    }

    fn wait_with_updates<'a>(
        &'a self,
        _input: &'a str,
        _context: ToolContext<'a>,
        _observer: &'a mut dyn CodeModeObserver,
    ) -> HostFuture<'a, Result<CodeModeExecution, CodeModeHostError>> {
        Box::pin(async {
            Err(CodeModeHostError::new(
                "embedded host does not support resumable cells",
            ))
        })
    }

    fn tool_definitions(&self, session_id: &str) -> Result<Vec<ToolDefinition>, CodeModeHostError>;

    fn execute<'a>(
        &'a self,
        source: &'a str,
        context: ToolContext<'a>,
    ) -> HostFuture<'a, Result<CodeModeExecution, CodeModeHostError>>;

    fn execute_with_updates<'a>(
        &'a self,
        source: &'a str,
        context: ToolContext<'a>,
        observer: &'a mut dyn CodeModeObserver,
    ) -> HostFuture<'a, Result<CodeModeExecution, CodeModeHostError>> {
        Box::pin(async move {
            let execution = self.execute(source, context).await?;
            for call in &execution.nested_calls {
                observer.update(CodeModeUpdate::NestedCallStarted {
                    call_id: &call.call_id,
                    name: &call.name,
                    input: &call.input,
                });
                observer.update(CodeModeUpdate::NestedCallCompleted(call));
            }
            Ok(execution)
        })
    }

    fn execute_with_local_tools<'a>(
        &'a self,
        source: &'a str,
        context: ToolContext<'a>,
        tools: Vec<std::sync::Arc<dyn crate::Tool>>,
        observer: Option<&'a mut dyn CodeModeObserver>,
    ) -> HostFuture<'a, Result<CodeModeExecution, CodeModeHostError>> {
        Box::pin(async move {
            if !tools.is_empty() {
                return Err(CodeModeHostError::new(
                    "embedded host does not support nested local tools",
                ));
            }
            match observer {
                Some(observer) => self.execute_with_updates(source, context, observer).await,
                None => self.execute(source, context).await,
            }
        })
    }

    fn execute_tool<'a>(
        &'a self,
        name: &'a str,
        _input: ToolInput,
        _context: ToolContext<'a>,
    ) -> HostFuture<'a, Result<ToolOutput, CodeModeHostError>> {
        Box::pin(async move {
            Err(CodeModeHostError::new(format!(
                "direct embedded tool `{name}` is unavailable"
            )))
        })
    }

    fn begin_turn(&self, _session_id: &str) {}

    fn detach_turn(
        &self,
        _session_id: &str,
        _observer: &mut (dyn FnMut(&str) -> Box<dyn CodeModeObserver> + Send),
    ) {
    }

    fn preempt_turn<'a>(
        &'a self,
        _session_id: &'a str,
    ) -> HostFuture<'a, Result<(), CodeModeHostError>> {
        Box::pin(async { Ok(()) })
    }

    fn settle_preempt(&self, _session_id: &str) {}

    fn cancel_turn<'a>(
        &'a self,
        session_id: &'a str,
    ) -> HostFuture<'a, Result<(), CodeModeHostError>> {
        self.cancel(session_id)
    }

    fn cancel_turn_with_updates<'a>(
        &'a self,
        session_id: &'a str,
        _observer: &'a mut dyn CodeModeObserver,
    ) -> HostFuture<'a, Result<(), CodeModeHostError>> {
        self.cancel_turn(session_id)
    }

    fn cancel<'a>(&'a self, _session_id: &'a str) -> HostFuture<'a, Result<(), CodeModeHostError>> {
        Box::pin(async { Ok(()) })
    }
}
