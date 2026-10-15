# Code Mode execution worker

This crate owns the native QuickJS worker used by `nanocode-code-mode`. It has
no MCP, OAuth, HTTP, media-decoding, backend or agent-runtime dependency. Separating
the worker allows it to compile while the Code Mode facade's other dependencies
are still building. Existing Code Mode APIs continue to use this worker.

`EmbeddedHost::spawn` starts one worker thread. `start_cell` evaluates one cell
with its tool definitions and stored JSON values. `read_event` yields tool calls,
content, notifications, explicit yields, completion or script failure.
`send_tool_result` resolves or rejects a nested tool call. `terminate` interrupts
JavaScript and waits for the worker to exit; dropping the host signals shutdown.
Each cell receives a fresh JavaScript context. Code Mode owns cell admission,
tool execution, durable receipts and observation; the worker runs one cell at a
time and uses caller-supplied cell and tool-call identifiers.

All native dependencies and worker APIs are gated out on WASM. WASM execution
continues through Code Mode's existing Rust host interface. This crate adds no
JavaScript bindings or wasm-bindgen interface.

The worker and its three bootstrap sources were moved from Code Mode without
behavior changes. Native worker checks exercise nested tool success/rejection,
storage, notifications, yielding, timers, fresh contexts and interruption.
