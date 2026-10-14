import init, { ChatGptSubscription as WasmChatGptSubscription } from "../pkg-web/nanocodex.js";

import { installHostBridge } from "../internal.mjs";
import { openSubscription } from "../runtime/chatgpt-subscription.mjs";

let initialized;


export async function open(options) {
  installHostBridge();
  initialized ||= init(options?.module === undefined
    ? undefined
    : { module_or_path: options.module }).catch((error) => {
      initialized = undefined;
      throw error;
    });
  await initialized;
  return openSubscription(options, (config) => WasmChatGptSubscription.open(config));
}
