# Development

- Do not add code comments, doc comments, or comment headers.
- Do not add project Cargo features or `cfg(feature = ...)` branches. Use crate boundaries and native/WASM target gates.
- Follow [the crate layout](crates/README.md): active orchestration in `runtime/`, provider code in `backends/`, shared contracts in `core/`, and host adapters in `integrations/`. Keep each backend's API definitions and tools in its own crate.
- Maintain one Rust crate workspace with reusable crates that compile individually for native and WASM targets. Keep language bindings, separately packaged WASM SDKs, managed services, and apps outside this repo. Do not add wasm-bindgen glue.
- Preserve agreed public APIs. Use upstream Claude/Codex prompts and tools; share Code Mode and CUA across both backends.
- Preserve native and WASM parity for agents, subagents, durability, and shared runtime contracts, including existing spawn/events, cancellation, and checkpoint/resume behavior. Keep host dependencies behind Rust platform interfaces; support WASM-local futures. Validate affected crate builds on native and WASM targets.
- Keep the dev loop fast: build only what changed, reuse caches, and avoid redundant checks. Honor requests to skip builds or CI.
- Benchmark performance changes with reproducible commands, inputs, and before/after results. Separate cold builds, incremental builds, and runtime performance.
- [Principles for fast Tokio applications](https://dial9-rs.github.io/blog/principles-for-fast-tokio-applications/)
- Keep logs, traces, and benchmark artifacts in ignored output/. Never commit credentials or personal test data.
- Prefer simple code and small changes. Preserve unrelated work. Use subagents only when explicitly requested.
- Avoid magic constants, arbitrary caps, and speculative defensive checks. Limits must come from a real protocol constraint, measured resource requirement, or explicit caller policy; document their source. Surface invalid states instead of hiding them with silent truncation, clamping, or fallback values.
- Work in the main checkout by default. Use a worktree only for an actual conflict or an explicit request; unrelated dirty or untracked files alone are not a reason.
