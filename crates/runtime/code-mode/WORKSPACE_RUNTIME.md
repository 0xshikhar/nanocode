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

Runner catalogs also contribute dynamic MCP tools to this shared namespace.
The same tool name selects a local provider by default in alongside mode or its
remote provider through `workdir`. Provider arguments do not receive this routing
field. CUA `mcp__cua_repl__js` and `mcp__cua_repl__reset` always require an explicit
`workdir` selecting a runner with a desktop provider. Calling `js` with only
`workdir` returns that desktop's tool contract without executing it. A root
without a desktop returns an error.

Logical process handles are partitioned by agent session ID and retain their
runner namespace and executor runtime identity. `ProcessRouteStore` supplies
`load` and `save` for these mappings; configure it through
`ToolsBuilder::process_route_store` or `Tools::with_process_route_store` when
composing Code Mode directly. The native SDK supplies an adapter using its
configured durable store. Reopening a durable session can poll its original
remote handles while that runner remains alive. A restarted runner or a local
process from a previous SDK runtime is fenced explicitly.

Reconnections recover existing calls from the same executor runtime's journal.
If the executor restarts, sent calls return an unknown-outcome error instead of
being replayed. New calls resume on the replacement connection. Cancellation
and turn-end frames are retained across connection gaps. A catalog timeout equal
to JavaScript's maximum safe integer denotes the existing unbounded timeout;
its wire deadline is that maximum timestamp. The SDK unbounded output token budget maps to the same safe-integer sentinel,
and zero output budgets remain zero. Other timeouts and budgets must
fit the wire protocol's integer range.

`RemoteRunner::with_call_timeout(Duration)` sets a caller policy applied to every
call in addition to the catalog deadline; the earlier deadline wins. Omitting
the policy preserves the catalog deadline. Durations must be positive whole
milliseconds within the protocol integer range. Expiration returns
`AttachmentError::Timeout` with the call ID and its outcome: `Ambiguous` after
dispatch or `Unavailable` before dispatch. Dispatched calls may have executed
and must not be retried automatically.

These socket entry points are native-only. WASM hosts continue to supply their
transport and local execution through the embedded Code Mode host contract.
