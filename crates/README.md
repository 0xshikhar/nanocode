# Crate layout

| Directory | Responsibility | Expected changes |
| --- | --- | --- |
| `nanocode/` | Public SDK composition and builders | Public API wiring |
| `runtime/` | Agent lifecycle, Code Mode, durability, and subagent orchestration | Active runtime development |
| `backends/` | Provider APIs, execution adapters, prompts, backend-owned tools, and each backend's durability adapter | Provider-specific maintenance |
| `core/` | Shared types, tool contracts, macros, media, TLS, and home configuration | Changes to shared contracts or infrastructure |
| `integrations/` | Computer use, observability, voice protocol, and state stores | Integration-specific development |
| `tui/` | SDK-independent terminal UI components (`nanocode-tui`): terminal session, composer, transcript, markdown, diff, pickers, queue, agent tree | Terminal presentation |
| `../bin/nanocodex/` | The `nanocodex` / `nc` interactive agent built on `nanocode` and `nanocode-tui` | Application behavior |

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
same traits. `SessionStore::register_format` registers a format on every clone of
the store; the SDK runtime registers the Codex, Claude, and configured backend
formats on the store passed to `.durability(...)`, which accepts any `StateStore`.

Durability is enabled by default on the SDK runtime. Native sessions use
`nanocode-store-jsonl` at the configured Codex home under `nanocode/sessions.jsonl`.
`.durability(...)` selects one shared store for roots and their subagents.
`nanocode-store-sqlite` and `nanocode-store-postgres` are optional store crates.
WASM defaults to memory; hosts implement `StateStore` over persistent storage to
retain sessions across host restarts.

## Composition

| Host | Crates |
| --- | --- |
| Native TUI or CLI | `nanocode`, `nanocode-tui`, a store such as `nanocode-store-jsonl` or `nanocode-store-sqlite`, optionally `nanocode-observability` |
| Native server | `nanocode`, `nanocode-store-postgres`, `nanocode-observability` |
| WASM worker | `nanocode` and a host `StateStore` |
| Browser JS | `nanocode-wasm` |
