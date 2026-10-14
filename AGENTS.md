# Development

- Build Rust and JavaScript/WASM SDKs. Keep managed services and apps outside this repo.
- Preserve agreed public APIs. Use upstream Claude/Codex prompts and tools; share Code Mode and CUA across both backends.
- Test user-visible behavior end to end through public APIs and real transports. Mock only unavoidable external dependencies.
- Keep the dev loop fast: build and test only what changed, reuse caches, and avoid redundant checks. Honor requests to skip builds or CI.
- Benchmark performance changes with reproducible commands, inputs, and before/after results. Separate cold builds, incremental builds, and runtime performance.
- Keep logs, traces, and benchmark artifacts in ignored output/. Never commit credentials or personal test data.
- Prefer simple code and small changes. Preserve unrelated work. Use subagents only when explicitly requested.
- Work in the main checkout by default. Use a worktree only for an actual conflict or an explicit request; unrelated dirty or untracked files alone are not a reason.
