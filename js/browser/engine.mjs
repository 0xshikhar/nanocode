import init from "../pkg-web/nanocodex.js";

let initialized;
let engine;


export function initializeBrowserEngine(options = {}) {
  return initialized ||= (options.module === undefined
    ? init()
    : init({ module_or_path: options.module })).then((exports) => (engine = exports)).catch((error) => {
      initialized = undefined;
      throw error;
    });
}



export function engineMemoryBytes() {
  return engine?.memory?.buffer?.byteLength;
}
