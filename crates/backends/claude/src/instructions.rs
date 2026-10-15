const CODE_MODE: &str = include_str!("../prompts/code_mode.md");

pub(crate) fn default_system_blocks(code_only: bool) -> Vec<serde_json::Value> {
    let mut blocks = crate::upstream::system_blocks();
    if code_only {
        blocks.push(serde_json::json!({"type": "text", "text": CODE_MODE}));
    }
    blocks
}
