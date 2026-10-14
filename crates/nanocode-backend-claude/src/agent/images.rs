use crate::{
    ContentBlock, ToolResultContent,
    prompt::{IMAGE_URL_UNSUPPORTED, ImageResolution, prepare_base64},
};
use serde_json::{Value, json};

pub(super) async fn prepare_tool_images(results: &mut [ContentBlock], resolution: ImageResolution) {
    let images: Vec<&mut Value> = results
        .iter_mut()
        .filter_map(|result| match result {
            ContentBlock::ToolResult {
                content: ToolResultContent::Blocks(blocks),
                ..
            } => Some(blocks),
            _ => None,
        })
        .flatten()
        .filter(|block| {
            block["type"] == "image"
                && matches!(block["source"]["type"].as_str(), Some("base64" | "url"))
        })
        .collect();
    for block in images {
        if block["source"]["type"] == "url" {
            *block = json!({"type": "text", "text": IMAGE_URL_UNSUPPORTED});
            continue;
        }
        let data = match block["source"]["data"].take() {
            Value::String(data) => data,
            _ => String::new(),
        };
        match prepare_base64(&data, resolution).await {
            Ok((data, media_type)) => {
                block["source"]["data"] = data.into();
                block["source"]["media_type"] = media_type.into();
            }
            Err(omission) => *block = json!({"type": "text", "text": omission}),
        }
    }
}
