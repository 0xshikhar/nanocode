use std::sync::OnceLock;

use serde_json::Value;

pub const TOOL_DEFINITIONS: &str = include_str!("tools/upstream.json");

pub fn tool_value(name: &str) -> Value {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS
        .get_or_init(|| {
            serde_json::from_str(TOOL_DEFINITIONS).expect("verified upstream Claude tool contracts")
        })
        .iter()
        .find(|tool| tool["name"] == name)
        .expect("verified upstream Claude tool name")
        .clone()
}
