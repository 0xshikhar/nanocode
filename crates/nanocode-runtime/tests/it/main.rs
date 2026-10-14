#![allow(missing_docs)]

mod lifecycle;
mod model;

use nanocode_runtime::{SessionCheckpoint, session::SessionSnapshot};

/// Codex-native conversation carried by a checkpoint's payload.
fn conversation(checkpoint: &SessionCheckpoint) -> SessionSnapshot {
    serde_json::from_value(checkpoint.payload()["conversation"].clone())
        .expect("a Codex checkpoint with a committed conversation")
}

const fn main() {}
