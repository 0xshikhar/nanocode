#[cfg(not(target_family = "wasm"))]
mod audio;
#[cfg(not(target_family = "wasm"))]
mod native;

#[cfg(not(target_family = "wasm"))]
pub use native::{EmbeddedHost, RuntimeEvent};
