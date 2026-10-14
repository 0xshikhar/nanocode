import type { ResponsesTransport } from "../browser/Transport.mjs";
import type * as Runtime from "../runtime/nanocodex.mjs";

export type Options = Runtime.Options<ResponsesTransport>;
export type CodexBackend = Runtime.CodexBackend<ResponsesTransport>;
export type {
  RunnerOptions,
  RunnerServerOptions,
  RunnerHandle,
  RunnerServer,
  RunnerEvent,
  ClaudeBackend,
  Defaults,
  ResumeOptions,
  Runtime,
  SpawnOptions,
  BackendRuntime,
  BackendSpawnOptions,
  InheritedSpawnOptions,
  ToolFactoryContext,
  ToolSource,
} from "../runtime/nanocodex.mjs";


export function create(options: Options): Promise<Runtime.Runtime>;
