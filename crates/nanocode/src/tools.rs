pub use nanocode_backend_codex::tools::*;

#[cfg(not(target_family = "wasm"))]
pub use nanocode_tool_runtime::{
    ForegroundOutput, ambient_sensitive_environment, mcp, remote, run_bash, shell,
};

#[cfg(not(target_family = "wasm"))]
pub mod attachment {
    pub use nanocode_tool_runtime::attachment::*;

    use nanocode_tool_runtime::Tools;

    pub fn run_runner(
        config: RunnerConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AttachmentError>> + Send>>
    {
        Box::pin(async move {
            let tools = Tools::builder()
                .without_defaults()
                .build()
                .map_err(|error| AttachmentError::Catalog(error.to_string().into()))?;
            run_runner_with_tools(config, tools).await
        })
    }

    pub fn run_runner_with_tools(
        mut config: RunnerConfig,
        tools: Tools,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AttachmentError>> + Send>>
    {
        Box::pin(async move {
            config.workspace = tokio::fs::canonicalize(&config.workspace)
                .await
                .map_err(|error| AttachmentError::Transport(error.to_string().into()))?;
            let tools = tools
                .into_builder()
                .workspace(false)
                .web_search(false)
                .image_generation(false)
                .add(nanocode_backend_codex::tools::WorkspaceTools::new(
                    config.workspace.clone(),
                ))
                .build()
                .map_err(|error| AttachmentError::Catalog(error.to_string().into()))?;
            nanocode_tool_runtime::attachment::run_runner_with_tools(config, tools).await
        })
    }
}
