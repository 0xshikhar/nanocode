use nanocode_tools::ToolOutputBody;
use nanocode_types::{PromptInput, responses::ContentItem};

/// Prepares embedded input with the shared image decoder, without local file access.
#[allow(
    clippy::unused_async,
    reason = "embedded image preparation runs inline"
)]
pub async fn prepare_user_input(input: &PromptInput) -> Vec<ContentItem> {
    crate::image::prepare_embedded_user_input(input)
}

/// Validates and normalizes embedded tool images without requiring a Tokio runtime.
#[allow(
    clippy::unused_async,
    reason = "embedded image preparation runs inline"
)]
pub async fn prepare_output_images(output: &mut ToolOutputBody) {
    crate::image::prepare_embedded_output_images(output);
}
