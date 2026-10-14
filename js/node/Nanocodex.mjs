import { connect as connectCua } from "./cua.mjs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { open as openWorkspace } from "./workspace.mjs";

import { create as createAgent } from "./Agent.mjs";
import { createNanocodexFactory } from "../runtime/nanocodex.mjs";

/** Configures backends once; every spawn is an independent Node WASM root Agent. */
export const create = createNanocodexFactory({
  placement: "node",
  connectCua,
  createAgent,
  async prepareWorkspace({ workspace, filesystem, needsFiles }) {
    if (filesystem) return { workspace: filesystem.root, filesystem };
    const path = resolve(workspace ?? process.cwd());
    return { workspace: path, filesystem: needsFiles ? await openWorkspace({ path, root: path }) : undefined };
  },
  async loadCatalog({ module }) {
    if (module === undefined) return createRequire(import.meta.url)("../pkg-node/nanocodex.js").modelCapabilities();
    const { initializeBrowserEngine } = await import("../browser/engine.mjs");
    await initializeBrowserEngine({ module });
    return (await import("../pkg-web/nanocodex.js")).modelCapabilities();
  },
});
