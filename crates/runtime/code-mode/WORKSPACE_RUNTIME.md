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

## Remote workspace runners

`attachment::run_runner(RunnerConfig { endpoint, token, workspace, name })`
runs a standalone native workspace executor. `RunnerEndpoint::Listen(address)`
accepts authenticated agent connections on a loopback WebSocket listener;
`RunnerEndpoint::Connect(url)` connects to an agent's authenticated `RunnerHost`.
Use a TLS proxy and `wss` when exposing a runner beyond loopback.
`run_runner_with_tools(config, tools)` also exports caller-supplied host adapters.

An agent connects with `RemoteRunner::new(name)` and
`runner.connect(AttachmentTarget::new(url, token)?)`. Keep the returned
`RunnerConnection` alive. For incoming runners, bind `RunnerHost`, register
`host.runner(name, token)`, and keep the host alive. Obtain the published catalog
with `runner.tools().await`, then configure
`Tools::builder().remote_runner(catalog, physical_workspace, mode)?.build()`.
Pass those tools to `CodexHarness::tools` or `ClaudeHarness::tools`.

`RemoteMode::Alongside` keeps the default cwd local. `/<name>` selects the
runner's physical workspace, and `/<name>/src` selects its `src` directory.
Multiple alongside runners may be registered. `RemoteMode::Replace` sends
workspace tool calls to its sole runner by default. Tool descriptions advertise
these mounts. Commands and file paths select their runner by mount; process
session IDs retain the selected runner for subsequent `write_stdin` calls.
Patch file headers must select one machine for the entire patch. Custom
function adapters receive an optional `workdir` routing field; routing consumes
that field before forwarding their own arguments.

Reconnections recover existing calls from the same executor runtime's journal.
If the executor restarts, sent calls return an unknown-outcome error instead of
being replayed. New calls resume on the replacement connection. Cancellation
and turn-end frames are retained across connection gaps. A catalog timeout equal
to JavaScript's maximum safe integer denotes the existing unbounded timeout;
its wire deadline is that maximum timestamp. The SDK unbounded output token budget maps to the same safe-integer sentinel,
and zero output budgets remain zero. Other timeouts and budgets must
fit the wire protocol's integer range.

These socket entry points are native-only. WASM hosts continue to supply their
transport and local execution through the embedded Code Mode host contract.
