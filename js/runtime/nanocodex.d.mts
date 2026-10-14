import type {
  BeforeCompactionRequest,
  CodeEffectJournal,
  CodeEvaluator,
  CompactionReceipt,
  DefaultAgent,
  DurabilityStore,
  ExecutionEnvironment,
  HarnessFamily,
  BackendModels,
  BackendName,
  BackendSelection,
  McpServers,
  ReasoningMode,
  SessionCheckpoint,
  Thinking,
  ToolConfiguration,
} from "../types.mjs";
import type { ClaudeTransport } from "./claude-transport.mjs";
import type { Tool as ClaudeTool } from "./claude.mjs";
import type { Tool as SubagentTool } from "./subagents.mjs";
import type { Workspace } from "./workspace.mjs";


export type ToolFactoryContext = Readonly<{ harness: HarnessFamily; role: "root" | "child" }>;

export type ToolSource<Tools> = Tools | ((context: ToolFactoryContext) => Tools | Promise<Tools>);

export type CodexBackend<Transport> = Readonly<{
  transport: Transport;

  tools?: ToolSource<ToolConfiguration<SubagentTool>> | undefined;
  reasoningMode?: ReasoningMode | undefined;
  fastMode?: boolean | undefined;
  instantToolSteering?: boolean | undefined;
  rawApiEvents?: boolean | undefined;
  additionalInstructions?: string | undefined;
  mcp?: McpServers | false | undefined;
  executionEnvironment?: ExecutionEnvironment | undefined;
  beforeCompaction?: ((request: BeforeCompactionRequest) => Promise<CompactionReceipt>) | undefined;
  codeEffectJournal?: CodeEffectJournal | undefined;
}>;

export type ClaudeBackend = Readonly<{
  transport: ClaudeTransport;

  tools?: ToolSource<readonly ClaudeTool[]> | undefined;
  maxTokens?: number | undefined;
  cache?: "off" | "5m" | "1h" | undefined;
  adaptiveThinking?: boolean | undefined;
  keepThinking?: boolean | undefined;
  fastMode?: boolean | undefined;
  parallelTools?: boolean | undefined;
  contextWindowTokens?: number | undefined;
  autoCompactWindowTokens?: number | undefined;
  subagents?: Readonly<{ maxConcurrency?: number | undefined }> | undefined;
}>;

export type Options<Transport> = Readonly<{

  backends: Readonly<{
    codex?: Transport | CodexBackend<Transport> | undefined;
    claude?: ClaudeTransport | ClaudeBackend | undefined;
  }>;

  thinking?: Thinking | undefined;
  instructions?: string | undefined;

  workspace?: string | undefined;

  filesystem?: Workspace | undefined;

  cua?: Readonly<{ tools: readonly import("../types.mjs").NamedTool[] }> | false | undefined;

  durability?: DurabilityStore | undefined;
  codeEvaluator?: CodeEvaluator | undefined;

  module?: unknown;
}> & (BackendSelection | Readonly<{ harness?: undefined; model?: undefined }>);

type CommonSpawnOptions = Readonly<{

  sessionId?: string | undefined;
  instructions?: string | undefined;
  thinking?: Thinking | undefined;
}>;
export type InheritedSpawnOptions = CommonSpawnOptions & Readonly<{ harness?: never; model?: never }>;
export type SpawnOptions<B extends BackendName = BackendName> = CommonSpawnOptions & BackendSelection<B>;
export type BackendSpawnOptions<B extends BackendName> = CommonSpawnOptions & Readonly<{
  model?: BackendModels[B] | undefined;
}>;
export type BackendRuntime<B extends BackendName> = Readonly<{
  spawn(options?: BackendSpawnOptions<B>): Promise<DefaultAgent<B>>;
}>;

export type ResumeOptions = Readonly<{

  harness?: HarnessFamily | undefined;
  instructions?: string | undefined;
}>;

export type Defaults = Readonly<{ harness: HarnessFamily; model: string; thinking: Thinking | undefined }>;

export type Runtime = Readonly<{
  placement: "node" | "host" | "browser";
  defaults: Defaults;
  backends: readonly HarnessFamily[];

  spawn<B extends BackendName>(options: SpawnOptions<B>): Promise<DefaultAgent<B>>;
  spawn(options?: InheritedSpawnOptions): Promise<DefaultAgent>;

  forBackend<B extends BackendName>(backend: B): BackendRuntime<B>;

  resume(checkpoint: SessionCheckpoint, options?: ResumeOptions): Promise<DefaultAgent>;

  shutdown(): Promise<void>;
}>;
