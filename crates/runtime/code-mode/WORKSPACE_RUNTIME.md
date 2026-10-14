# Nanocodex workspace runtime

`workspace_runtime::WorkspaceToolRuntime` executes `exec_command`, `write_stdin`,
`apply_patch`, and `view_image` through the same handlers as the native tool
registry. Interactive shell sessions remain alive for the lifetime of the
runtime. `WorkspaceToolRuntimeControl` terminates their process groups and
descendants.

The native crate includes the workspace runtime, Code Mode, and MCP together.
Platform gates select the native or embedded implementation; there are no Cargo
feature switches.

`standard` contains tool identities and definitions. Shared model-visible tool
contracts live in `nanocode-tools` and wire types in `nanocode-types`.
