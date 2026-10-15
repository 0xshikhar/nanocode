pub use crate::client::ImageDetail;
pub use nanocode_tools::{
    Tool, ToolContext, ToolInput, ToolOutput, ToolOutputBody, ToolOutputContent, ToolResult,
};

pub mod contract {
    pub use nanocode_tools::*;
}
pub use nanocode_media::image;

mod runtime_config;

pub use runtime_config::{ImageGenerationConfig, WebSearchConfig};
#[cfg(not(target_family = "wasm"))]
pub mod image_generation;
#[cfg(not(target_family = "wasm"))]
pub mod web_search;

pub use nanocode_tools::tool;

#[doc(hidden)]
pub use nanocode_tools::__private;

pub fn tool_runtime(
    workspace: impl AsRef<std::path::Path>,
    config: &crate::client::tower::ResponsesServiceConfig,
    tools: &nanocode_code_mode::Tools,
) -> nanocode_code_mode::runtime::ToolRuntime {
    let workspace = workspace.as_ref();
    #[cfg(not(target_family = "wasm"))]
    {
        nanocode_code_mode::runtime::ToolRuntime::new_with_tools(
            workspace,
            handlers(
                tools.web_search_enabled().then(|| WebSearchConfig {
                    endpoint: config.search_endpoint(),
                    auth: config.auth.clone(),
                }),
                tools
                    .image_generation_enabled()
                    .then(|| ImageGenerationConfig {
                        api_base_url: config.api_base_url.clone(),
                        auth: config.auth.clone(),
                        save_root: workspace.to_path_buf(),
                    }),
                tools.remote_http_client().unwrap_or_default(),
            ),
            tools,
        )
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = config;
        nanocode_code_mode::runtime::ToolRuntime::new_with_tools(workspace, Vec::new(), tools)
    }
}

#[cfg(not(target_family = "wasm"))]
pub fn handlers(
    web: Option<WebSearchConfig>,
    image: Option<ImageGenerationConfig>,
    client: reqwest::Client,
) -> Vec<std::sync::Arc<dyn Tool>> {
    let mut tools: Vec<std::sync::Arc<dyn Tool>> = Vec::new();
    if let Some(config) = web {
        tools.push(std::sync::Arc::new(
            web_search::WebSearchHandler::with_client(config, client.clone()),
        ));
    }
    if let Some(config) = image {
        tools.push(std::sync::Arc::new(
            image_generation::ImageGenerationHandler::with_client(config, client),
        ));
    }
    tools
}
