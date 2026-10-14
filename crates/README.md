# Crate layout

| Directory | Responsibility | Expected changes |
| --- | --- | --- |
| `nanocode/` | Public SDK composition and builders | Public API wiring |
| `runtime/` | Agent lifecycle, Code Mode, durability, and subagent orchestration | Active runtime development |
| `backends/` | Provider APIs, execution adapters, prompts, backend-owned tools, and each backend's durability adapter | Provider-specific maintenance |
| `core/` | Shared types, tool contracts, macros, media, TLS, and home configuration | Changes to shared contracts or infrastructure |
| `integrations/` | Computer use, host execution, observability, voice, and state stores | Integration-specific development |

Each backend is one crate containing its API definitions, tools, and durability
adapter. `core/` depends on nothing above it; `runtime/` depends only on `core/`
and `runtime/`. `scripts/check-dependency-boundaries.mjs` enforces both.

Directory placement describes ownership. Cargo dependencies determine rebuilds;
keep backend details out of shared contracts and runtime-only changes out of backends.

## Durability

`nanocode-durability` owns the durable state machine, the `StateStore` contract,
`MemoryStore`, the session catalog, and the `CheckpointFormat` trait. It has no
backend or database dependencies and builds for native and WASM.

Each backend implements `DurableAgentExt`, `DurableBackendRegistration`, and a
`CheckpointFormat` in its own `durability` module. Third-party backends use the
same traits. `SessionStore::with_format` registers formats;
`nanocode::session_store` registers the Codex and Claude formats.

Stores are separate crates: `nanocode-store-sqlite` and `nanocode-store-postgres`.
WASM hosts implement `StateStore` over their own storage.

## Composition

| Host | Crates |
| --- | --- |
| Native TUI or CLI | `nanocode`, `nanocode-store-sqlite`, optionally `nanocode-observability` and `nanocode-voice` |
| Native server | `nanocode`, `nanocode-store-postgres`, `nanocode-observability` |
| WASM worker | `nanocode` and a host `StateStore` |
| Browser JS | `nanocode-wasm` |
