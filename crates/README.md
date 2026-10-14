# Crate layout

| Directory | Responsibility | Expected changes |
| --- | --- | --- |
| `nanocode/` | Public SDK composition and builders | Public API wiring |
| `runtime/` | Agent lifecycle, Code Mode, durability, and subagent orchestration | Active runtime development |
| `backends/` | Provider APIs, execution adapters, prompts, and backend-owned tools | Provider-specific maintenance |
| `core/` | Shared types, tool contracts, macros, media, TLS, and home configuration | Changes to shared contracts or infrastructure |
| `integrations/` | Computer use, host execution, observability, and voice | Integration-specific development |

Each backend is one crate containing its API definitions and tools. Shared runtime
contracts must remain independent of concrete backends. The SDK composes them.

Directory placement describes ownership. Cargo dependencies determine rebuilds;
keep backend details out of shared contracts and runtime-only changes out of backends.
