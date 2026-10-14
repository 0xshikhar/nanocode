#![allow(missing_docs)]

#[cfg(all(feature = "openai", feature = "tools", not(target_family = "wasm")))]
mod tool_macro;

#[cfg(all(feature = "durability", not(target_family = "wasm")))]
mod claude;

#[cfg(all(feature = "openai", feature = "tools", not(target_family = "wasm")))]
mod harness;

#[cfg(all(feature = "openai", feature = "tools", not(target_family = "wasm")))]
mod runtime;

#[cfg(all(feature = "openai", feature = "tools", not(target_family = "wasm")))]
mod default_tools;

const fn main() {}
