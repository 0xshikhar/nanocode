import type {
  ChatGptSubscriptionHandle,
  ChatGptSubscriptionOptions,
} from "../types.mjs";


export function open(options: ChatGptSubscriptionOptions): Promise<ChatGptSubscriptionHandle>;
