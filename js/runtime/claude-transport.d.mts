declare const claudeTransport: unique symbol;


export type ClaudeTransport = Readonly<{ [claudeTransport]: true }>;


type ClaudeEndpointOptions = Readonly<{

  endpoint?: string | undefined;

  fetch?: typeof globalThis.fetch | undefined;
}>;


export function anthropic(options: ClaudeEndpointOptions & (
  | Readonly<{ apiKey: string; headers?: never; subscription?: never; subscriptionIdentity?: never }>
  | Readonly<{ headers(): HeadersInit | Promise<HeadersInit>; apiKey?: never; subscription?: never; subscriptionIdentity?: never }>
  | Readonly<{

    subscription: import("../worker/ClaudeSubscription.mjs").Subscription;
    subscriptionIdentity?: import("./claude.mjs").Options["subscriptionIdentity"];
    apiKey?: never;
    headers?: never;
  }>
)): ClaudeTransport;
