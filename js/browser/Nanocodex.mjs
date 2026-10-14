import { create as createAgent } from "./Agent.mjs";
import { create as createInlineAgent } from "./InlineAgent.mjs";
import { open as openWorkspace } from "./workspace.mjs";
import { initializeBrowserEngine } from "./engine.mjs";
import { createNanocodexFactory } from "../runtime/nanocodex.mjs";

/**
 * Configures backends once. Roots using a shared Workspace run in this isolate.
 * Codex-only runtimes with explicit tools retain the package Worker path.
 */
export const create = createNanocodexFactory({
  placement: "browser",
  createAgent(options) {
    return options.filesystem ? createInlineAgent(options) : createAgent(options);
  },
  async prepareWorkspace({ workspace, filesystem, needsFiles }) {
    if (filesystem || !needsFiles) return { workspace, filesystem };
    const shared = await openWorkspace({ name: workspace ?? "default", root: workspace ?? "/workspace" });
    return { workspace: shared.root, filesystem: shared };
  },
  async loadCatalog({ module }) {
    await initializeBrowserEngine({ module });
    return (await import("../pkg-web/nanocodex.js")).modelCapabilities();
  },
});
