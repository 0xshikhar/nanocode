# nanocode-cua

Thin stdio MCP transport to an explicitly supplied upstream Sky provider. The
provider owns the JavaScript REPL, computer/browser APIs, tool descriptions,
schemas, and confirmation behavior. This crate discovers and forwards its
catalog; it does not install Sky or implement computer control.

Connect once and add the tools to the shared native Rust runtime:

```rust,no_run
use nanocode::{ComputerConfig, ComputerTools, Nanocodex, OpenAi};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let cua = ComputerTools::connect(ComputerConfig::new("/path/to/upstream-provider")).await?;
let nano = Nanocodex::builder()
    .openai(OpenAi::new(std::env::var("OPENAI_API_KEY")?)?)
    .cua(cua)
    .workspace(std::env::current_dir()?)
    .build()?;
# Ok(())
# }
```

`.cua(...)` adds the discovered `mcp__cua_repl__*` tools to both configured
backends, including spawned agents, alongside each backend's coding tools.
Code Mode exposes them through `exec`/`wait`. Configure the provider's actual
arguments and environment on `ComputerConfig`; the executable path above is a
placeholder, not a bundled launcher.

Each conversation gets a separate provider process and persistent REPL scope.
Catalog changes require reconnecting. Cancellation after dispatch reports
uncertain effects and requires a reset before further input; it does not replay
input. Provider resources are retained by the connected transport's handles.

The Node adapter is part of the main JS package:

```js
import { Cua } from "nanocodex/node";

const cua = await Cua.connect({ executable: "/path/to/upstream-provider" });
// Pass `cua` alongside `backends` in your Nanocodex runtime options.
// After nano.shutdown():
await cua.close();
```

There is no separate computer package, custom Sky host, provisioning service,
managed connection, or confirmation-policy override. Remote Hands are separate
from this local provider transport.
