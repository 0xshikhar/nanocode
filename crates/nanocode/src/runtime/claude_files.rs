//! The canonical Claude Code file tools as shared tools, so Claude Code Mode
//! nests them under `exec` with their Claude names, schemas and results.

use std::{path::PathBuf, sync::Arc, time::Duration};

use nanocode_claude_tools::{ClaudeWorkspaceFiles, ImageSource, ToolContent, ToolResultBlock};
use nanocode_code_mode::{
    SessionEnvironment, Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult,
    Tools, ToolsBuildError,
    contract::{ToolOutputContent, async_trait},
};
use nanocode_codex::ImageDetail;
use serde_json::Value;

/// One agent's canonical file tools and workspace Bash capability.
pub(super) fn selection(
    files: &Arc<ClaudeWorkspaceFiles>,
    workspace: PathBuf,
    session: SessionEnvironment,
) -> Result<Tools, ToolsBuildError> {
    let mut tools = Tools::builder().without_defaults();
    for schema in ClaudeWorkspaceFiles::definitions() {
        let name = schema["name"].as_str().unwrap_or_default().to_owned();
        let description = schema["description"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        tools = tools.tool(ClaudeFileTool {
            definition: ToolDefinition::function(
                name.clone(),
                description,
                schema["input_schema"].clone(),
            ),
            name,
            files: Arc::clone(files),
            session: session.clone(),
        });
    }
    tools
        .tool(ClaudeBashTool {
            bash: nanocode_claude_tools::ClaudeBash::new(LocalBash { workspace }),
            session,
        })
        .build()
}

struct ClaudeFileTool {
    name: String,
    definition: ToolDefinition,
    files: Arc<ClaudeWorkspaceFiles>,
    session: SessionEnvironment,
}

#[async_trait]
impl Tool for ClaudeFileTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, input: ToolInput, _context: ToolContext<'_>) -> ToolResult {
        let input: Value = input.decode_json()?;
        let output = match self
            .files
            .execute_output_in_session(&self.name, input, true, Some(self.session.clone()))
            .await
        {
            Ok(output) => output,
            Err(error) => return Ok(ToolOutput::error(error)),
        };
        let result = match output.content {
            ToolContent::Text(text) if output.is_error => ToolOutput::error(text),
            ToolContent::Text(text) => ToolOutput::text(text),
            ToolContent::Blocks(blocks) => {
                ToolOutput::content(blocks.into_iter().map(item).collect())
            }
        };
        Ok(match output.metadata {
            Some(metadata) => result.with_metadata(metadata),
            None => result,
        })
    }
}

/// Native Claude blocks as shared tool content; images keep their bytes.
fn item(block: ToolResultBlock) -> ToolOutputContent {
    match block {
        ToolResultBlock::Text { text } => ToolOutputContent::InputText { text },
        ToolResultBlock::Image {
            source: ImageSource::Base64 { media_type, data },
        } => ToolOutputContent::InputImage {
            image_url: format!("data:{media_type};base64,{data}"),
            detail: ImageDetail::Auto,
        },
        ToolResultBlock::Image {
            source: ImageSource::Url { url },
        } => ToolOutputContent::InputImage {
            image_url: url,
            detail: ImageDetail::Auto,
        },
        ToolResultBlock::Document { .. } => ToolOutputContent::InputText {
            text: "[document content is available to direct Claude tools, not Code Mode]".into(),
        },
        ToolResultBlock::UnsupportedMedia { media_type } => ToolOutputContent::InputText {
            text: format!("[unsupported media omitted: {media_type}]"),
        },
    }
}

struct LocalBash {
    workspace: PathBuf,
}

impl nanocode_claude_tools::BashExecutor for LocalBash {
    async fn execute(
        &self,
        request: nanocode_claude_tools::BashRequest,
    ) -> Result<nanocode_claude_tools::BashResult, String> {
        let output = nanocode_code_mode::run_bash(
            &self.workspace,
            &request.command,
            Duration::from_millis(request.timeout_ms),
            request.max_stdout_bytes,
            request.max_stderr_bytes,
            request.session.as_ref(),
        )
        .await?;
        Ok(nanocode_claude_tools::BashResult {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.exit_code,
            truncated: output.truncated,
        })
    }
}

struct ClaudeBashTool {
    bash: nanocode_claude_tools::ClaudeBash<LocalBash>,
    session: SessionEnvironment,
}

#[async_trait]
impl Tool for ClaudeBashTool {
    fn definition(&self) -> ToolDefinition {
        let schema = nanocode_claude_tools::ClaudeBash::<LocalBash>::definitions().remove(0);
        ToolDefinition::function(
            "Bash",
            schema["description"].as_str().unwrap_or_default(),
            schema["input_schema"].clone(),
        )
    }
    async fn execute(&self, input: ToolInput, _context: ToolContext<'_>) -> ToolResult {
        let input = input.decode_json::<Value>()?;
        Ok(
            match self
                .bash
                .execute_in_session("Bash", input, Some(self.session.clone()))
                .await
            {
                Ok(output) => ToolOutput::text(output),
                Err(error) => ToolOutput::error(error),
            },
        )
    }
}
