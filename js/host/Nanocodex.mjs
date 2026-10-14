import { connectRunnerSocket } from "../browser/runners.mjs";
import { create as createAgent } from "../browser/InlineAgent.mjs";
import { initializeBrowserEngine } from "../browser/engine.mjs";
import { createNanocodexFactory } from "../runtime/nanocodex.mjs";


export const create = createNanocodexFactory({
  connectRunnerSocket,
  placement: "host",
  createAgent,
  requiresEvaluator: () => typeof globalThis.Worker !== "function",
  async loadCatalog({ module }) {
    await initializeBrowserEngine({ module });
    return (await import("../pkg-web/nanocodex.js")).modelCapabilities();
  },
});
