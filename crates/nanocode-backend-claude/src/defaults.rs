//! Backend-owned native default catalog and its per-session cleanup.
use nanocode_agent::{AgentHandle, NanocodexError, Result};
use nanocode_code_mode::{Tool, ToolContext, ToolDefinition, ToolInput, ToolResult, Tools};
use nanocode_tools::async_trait;
use std::{path::PathBuf, sync::Arc};

/// Installs canonical Claude file/Bash tools, or a caller's replacement selection,
/// plus additive shared tools (for example host-provided computer tools).
/// Background Bash jobs belong to this catalog and are joined during shutdown.
pub fn install_default_tools(
    workspace: PathBuf,
    handle: &AgentHandle,
    selected: Option<Tools>,
    additive: Vec<Arc<dyn Tool>>,
) -> Result<crate::ClaudeTools> {
    let jobs = Arc::new(crate::claude_bash::Jobs::default());
    let selected = match selected {
        Some(tools) => tools,
        None => {
            let files =
                nanocode_claude_tools::ClaudeWorkspaceFiles::new(&workspace).map_err(|error| {
                    NanocodexError::InvalidRequest(format!(
                        "Claude file tools require the runtime workspace {}: {error}",
                        workspace.display()
                    ))
                })?;
            crate::claude_files::selection(
                &Arc::new(files),
                workspace.clone(),
                handle.session_environment(),
                jobs.clone(),
            )
            .map_err(|error| NanocodexError::InvalidRequest(error.to_string()))?
        }
    };
    let mut builder = selected.into_builder();
    for tool in additive {
        builder = builder.tool(SharedTool(tool));
    }
    let selected = builder
        .build()
        .map_err(|error| NanocodexError::InvalidRequest(error.to_string()))?;
    Ok(
        crate::claude_tools::install(&selected, workspace, handle)?.adapter_shutdown(move || {
            let jobs = jobs.clone();
            async move {
                jobs.shutdown().await;
            }
        }),
    )
}

struct SharedTool(Arc<dyn Tool>);
#[async_trait]
impl Tool for SharedTool {
    fn definition(&self) -> ToolDefinition {
        self.0.definition()
    }
    async fn execute(&self, input: ToolInput, context: ToolContext<'_>) -> ToolResult {
        self.0.execute(input, context).await
    }
}
