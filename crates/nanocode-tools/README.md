# nanocode-tools

Shared `Tool`, `ToolContext`, `ToolInput` and typed-tool macros for Codex and
Claude. `nanocode-code-mode` executes either toolset through this contract.
Provider-specific adapters remain in `nanocode-codex-tools` and
`nanocode-claude-tools`.

Existing Responses wire data types are re-exported from `nanocode-codex` to
preserve their serialization and type identity. This crate owns invocation
traits; it does not own a provider connection or an execution runtime.
