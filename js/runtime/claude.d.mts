import type { DefaultAgent, DurabilityStore, SessionCheckpoint, ToolContext } from '../types.mjs';


export type Auth = Readonly<
  | { apiKey: string; headers?: never }
  | { headers(): HeadersInit | Promise<HeadersInit>; apiKey?: never }
>;
export type ToolContent = Readonly<
  | { type: 'input_text'; text: string }
  | { type: 'input_image'; image_url: string; detail?: 'auto' | 'low' | 'high' }
>;
export type NativeToolResult = Readonly<{
  content: string | readonly Record<string, unknown>[];
  isError?: boolean;
  structuredResult?: unknown;
  metadata?: unknown;
}>;
export type ToolResult = Readonly<{
  output: string | readonly ToolContent[];
  success: boolean;
  structuredResult?: unknown;
  metadata?: unknown;
}>;

export type Tool = Readonly<{
  name: string;
  description: string;
  inputSchema?: Record<string, unknown>;

  parameters?: Record<string, unknown>;
  strict?: boolean;
  deferLoading?: boolean;

  defer_loading?: boolean;

  supportsParallelToolCalls?: boolean;
  handler(input: unknown, context: ToolContext): unknown | Promise<unknown>;
}>;
export type CodexHarnessOptions = Readonly<{
  transport: import('../browser/Transport.mjs').ResponsesTransport | import('../node/Transport.mjs').ResponsesTransport;
  model?: import('../types.mjs').Model;
  thinking?: import('../types.mjs').Thinking;
  instructions?: string;
  workspace?: string;

  toolMode?: 'code-only';

  codeEvaluator?: import('../types.mjs').CodeEvaluator;
  tools?: import('../types.mjs').ToolConfiguration;
}>;
export type Options = Readonly<{
  harness?: 'claude';

  toolMode?: 'code-only';

  codeEvaluator?: import('../types.mjs').CodeEvaluator;

  subagents?: Readonly<{ maxConcurrency?: number }>;

  harnesses?: Readonly<{ codex?: CodexHarnessOptions }>;
  auth: Auth;

  model: import("../types.mjs").ClaudeModel | (string & {});
  endpoint?: string;

  fetch?: typeof globalThis.fetch;

  compatibilityProfile?: 'subscription';

  subscriptionIdentity?: Readonly<{ installId?: string; accountUuid?: string; userId?: string; platform?: string; arch?: string; version?: string }>;
  instructions?: string;
  systemBlocks?: readonly Record<string, unknown>[];

  sessionId?: string;
  workspace?: string;
  tools?: readonly Tool[];
  maxTokens?: number;
  thinking?: 'none' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';
  adaptiveThinking?: boolean;
  keepThinking?: boolean;

  fastMode?: boolean;
  cache?: 'off' | '5m' | '1h';
  parallelTools?: boolean;
  contextWindowTokens?: number;
  autoCompactWindowTokens?: number;

  autoCompact?: true;
  terminalReceiptRetention?: number;

  resume?: SessionCheckpoint;

  module?: unknown;
}> & (
  | { durability?: never; durabilityId?: never }
  | { durability: DurabilityStore; durabilityId: string }
);

export type Agent = DefaultAgent<"claude">;
