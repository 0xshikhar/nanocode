use nanocode_tool_contracts::ToolOutputBody;
use nanocode_types_core::{PromptInput, responses::ContentItem};

#[allow(
    clippy::unused_async,
    reason = "embedded image preparation runs inline"
)]
pub async fn prepare_user_input(input: &PromptInput) -> Vec<ContentItem> {
    crate::image::prepare_embedded_user_input(input)
}

#[allow(
    clippy::unused_async,
    reason = "embedded image preparation runs inline"
)]
pub async fn prepare_output_images(output: &mut ToolOutputBody) {
    crate::image::prepare_embedded_output_images(output);
}
