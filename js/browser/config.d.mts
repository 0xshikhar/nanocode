import type { DefaultAgent } from "../types.mjs";
import type { create as createAgent } from "./Agent.mjs";

export type AgentStatus = "idle" | "pending" | "success" | "error";

export type AgentSnapshot =
  | Readonly<{ data: undefined; error: undefined; status: "idle" }>
  | Readonly<{ data: undefined; error: undefined; status: "pending" }>
  | Readonly<{ data: DefaultAgent; error: undefined; status: "success" }>
  | Readonly<{ data: undefined; error: unknown; status: "error" }>;

export type AgentParameters = Readonly<{
  enabled?: boolean | undefined;
  threadId?: string | undefined;
}>;

export type Config = Readonly<{
  getAgent(parameters?: AgentParameters): AgentSnapshot;
  subscribeAgent(parameters: AgentParameters, listener: () => void): () => void;
  refetchAgent(parameters?: AgentParameters): void;
  destroy(): Promise<void>;
}>;

export type CreateConfigParameters = Readonly<{

  agent?: createAgent.Options | undefined;

  origin?: string | undefined;

  retry?: number | undefined;

  retryDelay?: ((attempt: number, error: unknown) => number) | undefined;
}>;


export function createConfig(options?: CreateConfigParameters): Config;
