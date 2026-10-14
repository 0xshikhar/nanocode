

export function create(options = {}) {
  if (options?.toolMode !== undefined && options.toolMode !== "code-only") throw new TypeError("toolMode must be code-only; direct tools are no longer supported");
  if (options.harness === 'claude') return import('./Claude.mjs').then(({ create }) => create(options));
  if (options.harness !== undefined && options.harness !== false && options.harness !== 'codex') throw new TypeError('unsupported harness family');
  if (options.harnesses !== undefined) return import('./InlineAgent.mjs').then(({ create }) =>
    create(options.harness === false ? { ...options, harness: 'codex' } : options));
  return import("./WorkerAgent.mjs").then(({ createWorkerAgent }) =>
    createWorkerAgent(options));
}
