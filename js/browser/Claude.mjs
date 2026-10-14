import { createClaude } from '../runtime/claude.mjs';
import { initializeBrowserEngine } from './engine.mjs';


export function create(options) {
  return createClaude(options, async (module) => {
    const wasm = await import('../pkg-web/nanocodex.js');
    await initializeBrowserEngine({ module });
    return wasm.Nanocodex;
  }, 'browser');
}
