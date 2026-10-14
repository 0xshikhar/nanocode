import type { ShellBinaryIO } from "./shell-binary.mjs";
import type { NamedTool, Workspace, ToolContext } from "./types.mjs";

export type JustBashExecutionEvent = Readonly<{
  phase: "queued" | "started" | "finished";

  command: string;
  command_scope: "first_literal";
  queue_ms?: number;
  duration_ms?: number;
  status?: "success" | "error";
  exit_code?: number | null;
  category?: "none" | "search_admission" | "resource_limit" | "timeout" | "cancelled" | "syntax" | "command_not_found" | "command_exit" | "input_validation" | "exception";

  admission_command?: "rg" | "grep" | "fgrep" | "egrep" | "sed" | "awk";
  output_truncated?: boolean;
}>;
export type JustBashExecutionObserver = (event: JustBashExecutionEvent, context?: ToolContext) => void;

export type JustBashFetch = (
  url: string,
  options?: Readonly<{
    method?: string | undefined;
    headers?: Headers | Record<string, string> | undefined;
    body?: string | undefined;
    followRedirects?: boolean | undefined;
    timeoutMs?: number | undefined;
    maxRedirects?: number | undefined;
    signal?: AbortSignal | undefined;
  }>,
) => Promise<Readonly<{
  status: number;
  statusText: string;
  headers: Record<string, string>;
  body: Uint8Array;
  url: string;
}>>;

export type JustBashCustomCommand = Readonly<{
  name: string;
  trusted?: boolean | undefined;
  execute(
    args: string[],
    context: unknown,
  ): Promise<Readonly<{ stdout: string; stderr: string; exitCode: number }>>;
}> | Readonly<{
  name: string;
  load(): Promise<JustBashCustomCommand>;
}>;

export type JustBashNetworkOptions = Readonly<{

  dangerouslyAllowFullInternetAccess?: boolean | undefined;

  allowedUrlPrefixes?: readonly string[] | undefined;

  allowedMethods?: readonly string[] | undefined;
}>;

export type JustBashRuntime = Readonly<{




  filesystem: Workspace;

  instructions: string;

  descriptor: JustBashDescriptor;

  tool: NamedTool;
}>;

export type JustBashDescriptor = Readonly<{
  shell: "nanocodex-just-bash";
  commands: readonly string[];
  customCommands: readonly string[];
  cwd: string;
  limits: Readonly<Record<string, number>>;
  network: Readonly<{ enabled: boolean; mode: string }>;
  pty: false;
  sessions: false;
  sandboxEscalation: false;
}>;







export function justBash(options: {

  filesystem: Workspace;

  refreshFilesystemBeforeExec?: boolean | undefined;

  lazyInitialize?: boolean | undefined;

  loadInterpreter?: (() => Promise<Pick<typeof import("just-bash/browser"), "Bash" | "defineCommand">>) | undefined;
  binaryIO?: ShellBinaryIO | undefined;

  onExecution?: JustBashExecutionObserver | undefined;
  executionTimeoutMs?: number | undefined;

  executionLimits?: Readonly<Record<string, number>> | undefined;
  maxEntries?: number | undefined;
  maxOutputTokens?: number | undefined;
  network?: false | JustBashNetworkOptions | undefined;

  networkMode?: string | undefined;

  fetch?: JustBashFetch | undefined;

  customCommands?: readonly JustBashCustomCommand[] | undefined;
}): Promise<JustBashRuntime>;
