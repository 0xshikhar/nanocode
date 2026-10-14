# Nanocode SDK

Rust SDK and JavaScript bindings through WebAssembly. Public APIs remain in the
`nanocode` facade; JavaScript bindings live directly in `js/`.

| Crate | Responsibility |
| --- | --- |
| `nanocode` | Public SDK facade and multi-harness configuration |
| `nanocode-runtime` | Shared agent lifecycle, turns, sessions and branching |
| `nanocode-codex` | Codex Responses client, authentication and transport |
| `nanocode-claude` | Claude Messages client and native agent loop |
| `nanocode-claude-tools` | Claude-native tools and host adapters |
| `nanocode-tools` | Shared Rust tool traits, invocation context and result contracts |
| `nanocode-code-mode` | Shared registry, Code Mode, MCP and remote execution |
| `nanocode-codex-tools` | Codex web search and image generation handlers |
| `nanocode-tools-macros` | Shared typed-tool procedural macros |
| `nanocode-cua` | Codex CUA/Node REPL provider adapter |
| `nanocode-hand` | Native capture, input and recording primitives |
| `nanocode-subagents` | Agent trees, messaging and result contracts |
| `nanocode-durability` | Checkpoint persistence and recovery |
| `nanocode-home` | Workspace instructions and skills discovery |
| `nanocode-observability` | Tracing and OpenTelemetry |
| `nanocode-voice-protocol` | Shared realtime protocol policy |

All Rust libraries are direct children of `crates/`. The WASM bridge crate lives
in `js/` alongside its JavaScript API. Package renaming does not change saved
credential locations, wire identifiers, or runtime type names.
