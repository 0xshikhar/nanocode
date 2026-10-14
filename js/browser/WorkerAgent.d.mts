import type { DefaultAgent } from "../types.mjs";
import type { create as createBrowserAgent } from "./Agent.mjs";

type WorkerAgentResourceOptions = Readonly<createBrowserAgent.CodexOptions & {

  origin?: string | undefined;
}>;
type WorkerAgentIdentityOptions = Pick<
  WorkerAgentResourceOptions,
  "accountConnectionRequests" | "module" | "origin" | "sessionId" | "threadId"
>;
type WorkerAgentPreparationOptions =
  | Readonly<WorkerAgentIdentityOptions & { harness: false }>
  | Readonly<WorkerAgentIdentityOptions & { harness?: undefined } & (
      | { threadId: string }
      | { sessionId: string }
    )>;

export type WorkerLike = {
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: ((event: { message?: string }) => void) | null;
  onmessageerror: (() => void) | null;
  postMessage(message: unknown): void;
  terminate?(): void;
};

export type WorkerAgentOptions = Readonly<{
  worker?: WorkerLike | (() => WorkerLike) | undefined;
  workerFactory?: (() => WorkerLike) | undefined;
  maxPendingRpcs?: number | undefined;

  onFailure?: ((error: Error) => void) | undefined;

  signal?: AbortSignal | undefined;
}>;


export function createWorkerAgent(
  options?: WorkerAgentResourceOptions,
  workerOptions?: WorkerAgentOptions,
): Promise<DefaultAgent>;


export function prepareWorkerAgent(
  options: WorkerAgentPreparationOptions,
  workerOptions?: WorkerAgentOptions,
): Promise<void>;

export type WorkerAgentRuntime = Readonly<{ dispose(): void }>;
export type WorkerAgentScope = WorkerLike;
export type WorkerAgentRuntimeOptions = Readonly<{
  createAgent?: (options: import("../host/Agent.mjs").create.Options) => Promise<DefaultAgent> | DefaultAgent;

  createDurabilityStore?: () => import("../types.mjs").DurabilityStore;
  prewarmLocal?: (
    harness: false | { threadId: string; origin?: string | undefined },
    options: { module?: WebAssembly.Module | undefined },
  ) => Promise<void> | void;
}>;


export function installWorkerAgentRuntime(
  scope?: WorkerAgentScope,
  options?: WorkerAgentRuntimeOptions,
): WorkerAgentRuntime;
