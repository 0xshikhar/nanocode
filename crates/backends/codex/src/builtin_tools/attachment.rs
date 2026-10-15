pub use nanocode_tool_runtime::attachment::*;

use nanocode_tool_runtime::Tools;

pub async fn run_runner(config: RunnerConfig) -> Result<(), AttachmentError> {
    let tools = Tools::builder()
        .without_defaults()
        .build()
        .map_err(|error| AttachmentError::Catalog(error.to_string().into()))?;
    run_runner_with_tools(config, tools).await
}

pub async fn run_runner_with_tools(
    mut config: RunnerConfig,
    tools: Tools,
) -> Result<(), AttachmentError> {
    config.workspace = tokio::fs::canonicalize(&config.workspace)
        .await
        .map_err(|error| AttachmentError::Transport(error.to_string().into()))?;
    let tools = tools
        .into_builder()
        .workspace(false)
        .web_search(false)
        .image_generation(false)
        .add(super::WorkspaceTools::new(config.workspace.clone()))
        .build()
        .map_err(|error| AttachmentError::Catalog(error.to_string().into()))?;
    nanocode_tool_runtime::attachment::run_runner_with_tools(config, tools).await
}
