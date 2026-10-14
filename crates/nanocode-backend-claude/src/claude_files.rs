//! The canonical Claude Code file tools as shared tools, so Claude Code Mode
//! nests them under `exec` with their Claude names, schemas and results.

use std::{path::PathBuf, sync::Arc};

use nanocode_agent::input::ImageDetail;
use nanocode_claude_tools::{ClaudeWorkspaceFiles, ImageSource, ToolContent, ToolResultBlock};
use nanocode_code_mode::{
    SessionEnvironment, Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult,
    Tools, ToolsBuildError,
    contract::{ToolOutputContent, async_trait},
};
use serde_json::Value;

/// One agent's canonical file tools and workspace Bash capability.
pub(super) fn selection(
    files: &Arc<ClaudeWorkspaceFiles>,
    workspace: PathBuf,
    session: SessionEnvironment,
    jobs: Arc<super::claude_bash::Jobs>,
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
    super::claude_bash::install(tools, workspace, session, jobs).build()
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

    async fn execute(&self, input: ToolInput, context: ToolContext<'_>) -> ToolResult {
        let input: Value = input.decode_json()?;
        let output = match self
            .files
            .as_ref()
            .clone()
            .with_read_token_budget(context.output_token_budget().min(200_000))
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
