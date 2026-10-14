pub use crate::client::ImageDetail;
pub use nanocode_tools::{
    Tool, ToolContext, ToolInput, ToolOutput, ToolOutputBody, ToolOutputContent, ToolResult,
};

pub mod contract {
    pub use nanocode_tools::*;
}
pub use nanocode_media::image;
#[cfg(feature = "native")]
mod runtime_config;
#[cfg(feature = "native")]
pub use runtime_config::{ImageGenerationConfig, WebSearchConfig};
#[cfg(all(feature = "native", not(target_family = "wasm")))]
pub mod image_generation;
#[cfg(all(feature = "native", not(target_family = "wasm")))]
pub mod web_search;

pub use nanocode_tools::tool;

#[doc(hidden)]
pub use nanocode_tools::__private;

#[cfg(feature = "native")]
pub fn handlers(
    web: Option<WebSearchConfig>,
    image: Option<ImageGenerationConfig>,
    client: reqwest::Client,
) -> Vec<std::sync::Arc<dyn Tool>> {
    #[cfg(not(target_family = "wasm"))]
    {
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
    #[cfg(target_family = "wasm")]
    {
        let _ = (web, image, client);
        Vec::new()
    }
}
