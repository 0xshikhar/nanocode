# Nanocodex JavaScript SDK

Node, browser and worker bindings to the Rust runtime through WebAssembly.

Public entry points: `nanocodex/node`, `nanocodex/browser`, `nanocodex/host`,
`nanocodex/worker`, `nanocodex/tools`, `nanocodex/subagents` and `nanocodex/durability`.

Run `pnpm --dir js build` to generate the WASM bindings.
Provider transports and storage are application-owned. Managed-platform entry points have been removed.
