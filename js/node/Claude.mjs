import { createNodeEvaluator } from './code-evaluator.mjs';
import { createRequire } from 'node:module';
import { createClaude } from '../runtime/claude.mjs';
import { createNodeHost } from './host.mjs';
import { createBrowserHost } from '../browser/host.mjs';

function createCodexHost(options) {

  if (options.hostAuth || options.createResponse || options.createWebSocket || options.WebSocketImpl) {
    return createBrowserHost(options);
  }
  return createNodeHost(options);
}


export function create(options) {
  return createClaude(options, async (module) => {
    if (module === undefined) return createRequire(import.meta.url)('../pkg-node/nanocodex.js').Nanocodex;
    const wasm = await import('../pkg-web/nanocodex.js');
    const { initializeBrowserEngine } = await import('../browser/engine.mjs');
    await initializeBrowserEngine({ module });
    return wasm.Nanocodex;
  }, 'node', { createCodexHost, codeEvaluator: createNodeEvaluator() });
}
