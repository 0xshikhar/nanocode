//! Codex-specific tool implementations over the shared tool contract.
pub use nanocode_codex::ImageDetail;
pub use nanocode_tools::{
    Tool, ToolContext, ToolInput, ToolOutput, ToolOutputBody, ToolOutputContent, ToolResult,
};
/// Shared output contracts.
pub mod contract {
    pub use nanocode_tools::*;
}
pub mod image;
#[cfg(feature = "native")]
mod runtime_config;
#[cfg(feature = "native")]
pub use runtime_config::{ImageGenerationConfig, WebSearchConfig};
#[cfg(all(feature = "native", not(target_family = "wasm")))]
pub mod image_generation;
#[cfg(all(feature = "native", not(target_family = "wasm")))]
pub mod web_search;

/// Shared typed tool macro.
pub use nanocode_tools::tool;

#[doc(hidden)]
pub use nanocode_tools::__private;
