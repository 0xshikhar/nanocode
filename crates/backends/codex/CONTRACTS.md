# Codex API contracts

The Codex backend includes its API client, transport, execution adapter, and
backend-owned tools in one crate. Native and WASM platform adapters are selected
by target configuration; there are no Cargo feature switches.

Shared prompt, response-item, and tool wire contracts live in `nanocode-types`.
Shared tool interfaces live in `nanocode-tools`. Consumers needing only shared
contracts can depend on those crates directly.
