import type {
  ChatGptSubscriptionHandle,
  ChatGptSubscriptionOptions,
} from "../types.mjs";


export function open(
  options: Omit<ChatGptSubscriptionOptions, "module"> & { module: WebAssembly.Module },
): Promise<ChatGptSubscriptionHandle>;
