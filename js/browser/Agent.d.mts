import type {
  AgentLifecycle,
  AgentOptions,
  DefaultAgent,
  ExecutionEnvironment,
} from "../types.mjs";
import type { WorkerTransport } from "./Transport.mjs";
import type { Tools } from "../tools/Tools.mjs";

export type Agent = DefaultAgent;

type WorkerMcpServer = Readonly<{
  url?: string | URL | undefined;
  description?: string | undefined;
  headers?: Readonly<Record<string, string>> | readonly (readonly [string, string])[] | undefined;
  enabledTools?: readonly string[] | undefined;
  disabledTools?: readonly string[] | undefined;
  supportsParallelToolCalls?: boolean | undefined;
  parallelTools?: readonly string[] | undefined;
  startupTimeoutMs?: number | undefined;
  timeoutMs?: number | undefined;
}>;
type WorkerMcpServers = Readonly<Record<string, string | URL | WorkerMcpServer>>;

type WorkerToolExposureOptions =
  | { mcp?: false | undefined; toolMode?: "code-only" | undefined }
  | { mcp: WorkerMcpServers; toolMode?: "code-only" | undefined };


export function create(options: create.ClaudeOptions): Promise<DefaultAgent<"claude">>;
export function create(options?: create.CodexOptions): Promise<DefaultAgent<"codex">>;
export function create(options?: create.Options): Promise<create.ReturnType>;
export declare namespace create {

  type CodexOptions = Omit<AgentOptions, "beforeCompaction" | "harness"> & WorkerToolExposureOptions & {

    module?: WebAssembly.Module | undefined;

    executionEnvironment?: ExecutionEnvironment | undefined;

    transport?: WorkerTransport | undefined;

    threadId?: string | undefined;

    accountConnectionRequests?: boolean | undefined;

    durability?: false | undefined;

    harness?: false | 'codex' | undefined;
  };

  type ClaudeOptions = import('../runtime/claude.mjs').Options & { harness: 'claude' };

  type Options = CodexOptions | ClaudeOptions;
  type ReturnType = Agent;
}
