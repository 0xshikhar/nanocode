import type { DefaultAgent, BackendName, BackendSelection, ModelSelection, Thinking } from "../types.mjs";


type SubagentOwner = Omit<DefaultAgent, "extend"> | import("./claude.mjs").Agent;

declare const subagentToolBrand: unique symbol;


export type Tool = Readonly<{
  [subagentToolBrand]: true;
}>;

export type Subagents = readonly [Tool];

export interface Options {

  maxConcurrency?: number | undefined;

  continuation?: boolean | undefined;
}

export type AgentId = number;
export type AgentStatus =
  | Readonly<{ state: "pending" | "running" | "interrupted" | "closing" | "closed" }>
  | Readonly<{ state: "completed"; output: unknown }>
  | Readonly<{ state: "failed"; error: string }>;
export type AgentSummary = Readonly<{
  agent_id: AgentId;
  role: string;
  task: string;
  parent_agent_id: AgentId | null;
  status: AgentStatus;
  last_output?: unknown;
  completion_revision?: number;
}>;
export type JsonSchema = boolean | Readonly<Record<string, unknown>>;
export type SpawnOptions<B extends BackendName = BackendName> = Readonly<{
  role: string;
  task: string;
  thinking?: Thinking | undefined;
  outputSchema: JsonSchema;
}> & (BackendSelection<B>
  | Readonly<{ harness?: undefined; model?: ModelSelection<B> | undefined }>);

export type BatchSpawnOptions = Readonly<{
  role: string;
  task: string;
  outputSchema: JsonSchema;
}>;
export type SpawnReport = Readonly<{
  agent_id: AgentId;
  role: string;
  status: Readonly<{ state: "running" }>;
}>;
export type WaitOptions = Readonly<{
  agentIds: readonly AgentId[];
  timeoutMs?: number | undefined;
}>;
export type WaitReport = Readonly<{
  agents: readonly AgentSummary[];
  timed_out: boolean;
}>;
export type LifecycleReport = Readonly<{ agents: readonly AgentSummary[] }>;
export type DirectoryEntry = AgentSummary & Readonly<{
  can_message: boolean;
  can_manage: boolean;
  resuming?: true;
}>;
export type DirectoryOptions = Readonly<{
  includeCompleted?: boolean | undefined;
  includeSelf?: boolean | undefined;
}>;
export type DirectoryReport = Readonly<{ agents: readonly DirectoryEntry[] }>;
export type MessagePriority = "deferred" | "urgent";
export type MessagePurpose = "delegate" | "coordinate" | "finding" | "question" | "reply";
export type MessageSender =
  | Readonly<{ kind: "root" }>
  | Readonly<{ kind: "agent"; agent_id: AgentId }>;
export type SendOptions = Readonly<{
  agentId: AgentId;
  message: string;
  priority?: MessagePriority | undefined;
  purpose?: MessagePurpose | undefined;

  inReplyTo?: number | undefined;
}>;
export type MessageReceipt = Readonly<{
  message_id: number;
  thread_id: number;
  from: MessageSender;
  to_agent_id: AgentId;
  disposition: "started" | "queued" | "steered";
}>;


export function create(options?: Options): Subagents;

export function spawn(agent: SubagentOwner, options: SpawnOptions): Promise<SpawnReport>;

export function spawnMany(
  agent: SubagentOwner,
  options: readonly BatchSpawnOptions[],
): Promise<readonly SpawnReport[]>;

export function wait(agent: SubagentOwner, options: WaitOptions): Promise<WaitReport>;

export function list(agent: SubagentOwner, options?: DirectoryOptions): Promise<DirectoryReport>;

export function send(agent: SubagentOwner, options: SendOptions): Promise<MessageReceipt>;

export function interrupt(agent: SubagentOwner, agentId: AgentId): Promise<LifecycleReport>;

export function close(agent: SubagentOwner, agentId: AgentId): Promise<LifecycleReport>;

export {};
