import type { Options as ClaudeOptions } from './runtime/claude.mjs';
export type Thinking = "none" | "low" | "medium" | "high" | "xhigh" | "max";
export type ReasoningMode = "standard" | "pro";
export type Model = "gpt-6.1-sol" | "gpt-6-luna" | "gpt-6-astra" | "@cf/zai-org/glm-5.3" | "kimi-k3" | "mimo-v2.6-pro";

export type HarnessFamily = "codex" | "claude" | (string & {});

export type ClaudeModel =
  | "claude-opus-5-5"
  | "claude-sonnet-5-5"
  | "claude-haiku-5-5"
  | "claude-fable-5-1"
  | "claude-opus-4-6"
  | "claude-sonnet-4-6"
  | "claude-haiku-4-5";

export interface BackendModels {
  codex: Model;
  claude: ClaudeModel;
}
export type BackendName = keyof BackendModels;

export type ModelSelection<B extends BackendName = BackendName> = {
  [K in B]: Readonly<{ backend: K; model: BackendModels[K] }>
}[B];

export type BackendSelection<B extends BackendName = BackendName> = {
  [K in B]: Readonly<{ harness: K; model?: BackendModels[K] | undefined }>
}[B];


export type SessionOrigin = "root" | "fork" | "side_conversation" | "subagent" | "branch";


export type SessionLineage = Readonly<{

  rootSessionId: string;

  parentSessionId: string | null;
  origin: SessionOrigin;

  depth: number;
}>;


export type SessionInfo = Readonly<{
  sessionId: string;
  harness: HarnessFamily;
  lineage: SessionLineage;
}>;


export type ServiceTier = "standard" | "priority" | "fast" | "ultrafast";

export type ModelTransport = "native" | "managed";

export type ModelCapabilityEntry = Readonly<{
  model: string;
  family: HarnessFamily;
  transport: ModelTransport;

  thinking: readonly Thinking[];
  defaultThinking: Thinking;

  familyDefault: boolean;
  fastMode: boolean;

  serviceTiers: readonly Exclude<ServiceTier, "priority">[];
  reasoningModes: readonly ReasoningMode[];
}>;


export type Mutability = "fixed" | "before_first_prompt" | "anytime";






export type SessionCapabilities = Readonly<{
  checkpoint: boolean;

  resume: boolean;
  fork: boolean;

  forkAt: boolean;
  sideConversation: boolean;
  spawn: boolean;
  steering: boolean;
  identifiedSteering: boolean;
  compaction: boolean;
  developerMessages: boolean;
  context: boolean;
  model: Mutability;
  thinking: Mutability;
  serviceTier: Mutability;

  ultrafastServiceTier: boolean;
}>;


export type SessionPersistence = Readonly<{

  durableStateId: string | null;

  serverSessionId: string | null;

  resumable: boolean;
}>;

declare const sessionCheckpointBrand: unique symbol;







export type SessionCheckpoint = Readonly<{
  readonly [sessionCheckpointBrand]: "NanocodexSessionCheckpoint";
}>;

export type PromptItem =
  | { type: "text"; text: string }
  | { type: "image"; image_url: string; file_id?: never; detail?: "auto" | "low" | "high" | "original" | undefined }
  | { type: "image"; file_id: string; image_url?: never; detail?: "auto" | "low" | "high" | "original" | undefined }
  | { type: "audio"; audio_url: string }

  | { type: "file"; file_data: string; filename?: string | undefined };

export type PromptInput = string | readonly PromptItem[];

export type AgentEvent = {
  protocol_version: number;
  request_id: string;
  seq: number;
  type: string;
  payload: Record<string, unknown>;
};


export type BeforeCompactionRequest = Readonly<{

  boundaryId: string;
  sessionId: string;
  rootSessionId: string;

  messages: readonly Readonly<{ role: "user" | "assistant"; text: string }>[];
  truncated: boolean;

  signal: AbortSignal;
}>;

export type CompactionReceipt = Readonly<{

  receiptId: string;
}>;

export type AgentOptions = {

  harness?: "codex" | undefined;

  harnesses?: Readonly<{ claude?: ClaudeOptions }> | undefined;





  beforeCompaction?: ((request: BeforeCompactionRequest) => Promise<CompactionReceipt>) | undefined;


  instructions?: string | undefined;

  additionalInstructions?: string | undefined;
  model?: Model | undefined;
  reasoningMode?: ReasoningMode | undefined;
  fastMode?: boolean | undefined;

  instantToolSteering?: boolean | undefined;

  rawApiEvents?: boolean | undefined;
  sessionId?: string | undefined;
  thinking?: Thinking | undefined;
  workspace?: string | undefined;







  resume?: SessionCheckpoint | undefined;
};


export type ExecutionEnvironment = Readonly<{
  currentDate: string;
  timezone: string;
  projectInstructions?: string | undefined;
}>;


declare const durabilityRevisionBrand: unique symbol;
export type DurabilityRevision = string & {
  readonly [durabilityRevisionBrand]: "NanocodexDurabilityRevision";
};


declare const durabilityFenceBrand: unique symbol;
export type DurabilityFence = string & {
  readonly [durabilityFenceBrand]: "NanocodexDurabilityFence";
};

export type DurabilityStoredState = Readonly<{
  revision: DurabilityRevision;
  payload: string | null;
}>;


export type DurabilityPortableStateArchive = DurabilityStoredState & Readonly<{
  format: "nanocodex-durability-state-v2";
  stateId: string;
  records: readonly DurabilityRecord[];
}>;

export type DurabilityExportCursor = string;


export type DurabilityPortableStatePage = Readonly<{
  format: "nanocodex-durability-state-page-v2";
  stateId: string;
  from: DurabilityRevision;

  fromDigest: string;
  to: DurabilityRevision;
  cursor: DurabilityExportCursor;
  nextCursor: DurabilityExportCursor | null;

  payloadLength: number;
  payload: string;
  records: readonly DurabilityRecord[];
}>;

export type DurabilityExportPageRequest = Readonly<{
  from: DurabilityRevision;

  fromDigest?: string | undefined;

  to?: DurabilityRevision | undefined;
  cursor?: DurabilityExportCursor | undefined;

  limit?: number | undefined;
}>;

export type DurabilityAcquireRequest = Readonly<{
  ownerId: string;
}>;

export type DurabilityAcquiredState = DurabilityStoredState & Readonly<{
  ownerId: string;
  fence: DurabilityFence;
}>;

export type DurabilityRecord = Readonly<{ key: string; value: string }>;

export type DurabilityReplaceRequest = Readonly<{
  records: readonly DurabilityRecord[];
  ownerId: string;
  fence: DurabilityFence;
  expectedRevision: DurabilityRevision;
  payload: string;
}>;

export type DurabilityReplaceResult =
  | Readonly<{ status: "replaced"; revision: DurabilityRevision }>
  | Readonly<{ status: "fenced" }>
  | Readonly<{ status: "conflict"; actualRevision: DurabilityRevision }>
  | Readonly<{ status: "not_committed"; message: string }>;


export type DurabilityStore = Readonly<{
  readRecord(stateId: string, key: string): string | null | Promise<string | null>;

  readRecords?(stateId: string, keys: readonly string[]): readonly (string | null)[] | Promise<readonly (string | null)[]>;
  load(stateId: string): DurabilityStoredState | Promise<DurabilityStoredState>;
  acquire(
    stateId: string,
    request: DurabilityAcquireRequest,
  ): DurabilityAcquiredState | Promise<DurabilityAcquiredState>;
  replace(
    stateId: string,
    request: DurabilityReplaceRequest,
  ): DurabilityReplaceResult | Promise<DurabilityReplaceResult>;
}>;


export type DurabilityPortableStore = DurabilityStore & Readonly<{
  scanRecords(stateId: string, after?: string, limit?: number): readonly DurabilityRecord[] | Promise<readonly DurabilityRecord[]>;
  importRecords(stateId: string, records: readonly DurabilityRecord[]): void | Promise<void>;
  importState(
    stateId: string,
    state: DurabilityStoredState,
    options?: Readonly<{
      records?: readonly DurabilityRecord[];
      expectedRevision?: DurabilityRevision | undefined;

      expectedPayload?: string | null | undefined;
    }> | undefined,
  ): DurabilityStoredState | Promise<DurabilityStoredState>;
}>;


export type MemoryDurabilityStore = DurabilityPortableStore & Readonly<{
  stateId: string;
  snapshot(): DurabilityStoredState;
}>;





export type DurabilitySqliteValue = string | number | null;
export type DurabilitySqliteRow = Record<string, DurabilitySqliteValue>;

export type DurabilitySqliteQuery = <Row extends DurabilitySqliteRow>(
  sql: string,
  args: readonly DurabilitySqliteValue[],
) => readonly Row[] | Promise<readonly Row[]>;

export type DurabilitySqliteTransaction = <Result>(
  callback: (query: DurabilitySqliteQuery) => Result | Promise<Result>,
) => Result | Promise<Result>;

export type SqliteDurabilityStoreOptions = Readonly<{
  transaction: DurabilitySqliteTransaction;
}>;


declare const subscriptionRevisionBrand: unique symbol;
export type SubscriptionRevision = string & {
  readonly [subscriptionRevisionBrand]: "NanocodexSubscriptionRevision";
};

export type SubscriptionStoredValue = Readonly<{
  revision: SubscriptionRevision;
  payload?: string | undefined;
}>;

export type SubscriptionCommitRequest = Readonly<{
  expectedRevision: SubscriptionRevision;

  payload: string;
}>;

export type SubscriptionCommitResult =
  | Readonly<{ status: "committed"; revision: SubscriptionRevision }>
  | Readonly<{ status: "conflict"; actualRevision: SubscriptionRevision }>;


export type ChatGptSubscriptionStore = Readonly<{
  load(id: string): SubscriptionStoredValue | Promise<SubscriptionStoredValue>;
  compareAndSwap(
    id: string,
    request: SubscriptionCommitRequest,
  ): SubscriptionCommitResult | Promise<SubscriptionCommitResult>;
}>;

export type MemoryChatGptSubscriptionStore = ChatGptSubscriptionStore & Readonly<{
  id: string;
  snapshot(): SubscriptionStoredValue;
}>;

export type ChatGptCredentialSeed = Readonly<{
  accessToken: string;
  refreshToken?: string | undefined;
  accountId: string;
  fedramp?: boolean | undefined;
}>;

export type ChatGptLoginStatus =
  | Readonly<{ state: "signed_out" | "expired" }>
  | Readonly<{
      state: "pending";
      verificationUrl: string;
      userCode: string;
      expiresAt: number;
      pollAfterMs: number;
    }>
  | Readonly<{
      state: "authenticated";
      accountId: string;
      expiresAt: number | null;
    }>;

export type ChatGptCredential = Readonly<{
  kind: "chatgpt";

  accessToken: string;
  accountId: string;
  fedramp: boolean;
  revision: SubscriptionRevision;
}>;

export type ChatGptSubscriptionHandle = Readonly<{
  id: string;
  startLogin(): Promise<ChatGptLoginStatus>;
  status(): Promise<ChatGptLoginStatus>;
  credential(): Promise<ChatGptCredential>;
  recover(rejectedRevision: SubscriptionRevision): Promise<ChatGptCredential>;
  logout(): Promise<void>;
  dispose(): void;
}>;

export type ChatGptSubscriptionOptions = Readonly<{
  id: string;
  store: ChatGptSubscriptionStore;

  fetch?: typeof globalThis.fetch | undefined;




  seed?: ChatGptCredentialSeed | undefined;

  issuer?: string | undefined;

  module?: unknown;
}>;

export type EstimatedUsdCost = Readonly<{
  usd: string;
  input_usd: string;
  cached_input_usd: string;
  cache_write_input_usd: string;
  output_usd: string;
  service_tier: ServiceTier;
}>;

export type CostStatus =
  | "estimated_from_usage"
  | "usage_not_reported"
  | "other";

export type TurnUsage = Readonly<{
  input_tokens: number;
  cached_input_tokens: number;
  cache_write_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
  estimated_cost: EstimatedUsdCost | null;
  cost_status: CostStatus;
}>;

export type ForkOptions = Readonly<{

  at?: TurnResult | SessionCheckpoint | undefined;

  origin?: "fork" | "side_conversation" | undefined;
}>;
export type WatchEventsOptions = { includeAllSessions?: boolean | undefined };


export type AgentSessionContext = Readonly<{
  workspace: string;
  history: readonly Record<string, unknown>[];
}>;

export type RealtimeTranscriptEntry = Readonly<{
  role: "user" | "assistant";
  text: string;
}>;

export type EventWatcher = Readonly<{
  onEvent(listener: (event: AgentEvent) => void): () => void;
  off(): void;
  [Symbol.asyncIterator](): AsyncIterableIterator<AgentEvent>;
}>;

export type AgentActions<B extends BackendName = never> = {
  events: {
    watch(options?: WatchEventsOptions): EventWatcher;
  };
  session: {

    info(): SessionInfo;

    capabilities(): SessionCapabilities;

    persistence(): SessionPersistence | null;

    checkpoint(): Promise<SessionCheckpoint>;
    appendDeveloperMessage(text: string): Promise<AgentSessionContext>;

    cancel(): Promise<void>;
    compact(): Promise<void>;
    context(): Promise<AgentSessionContext>;
    fork(options?: ForkOptions): Promise<DefaultAgent<B>>;

    setModel(model: BackendModels[B]): Promise<void>;

    setFastMode(enabled: boolean): Promise<void>;

    setServiceTier(serviceTier: ServiceTier): Promise<void>;
    setThinking(thinking: Thinking): Promise<void>;
    shutdown(): Promise<void>;
    spawn(): Promise<DefaultAgent<B>>;
    realtime: {
      start(): Promise<AgentSessionContext>;
      end(): Promise<AgentSessionContext>;
      delegation(
        input: string,
        transcript?: readonly RealtimeTranscriptEntry[],
      ): Promise<string>;
      tailDelegation(
        transcript: readonly RealtimeTranscriptEntry[],
      ): Promise<string | undefined>;
    };
  };
  turn: {
    prompt(options: {
      input: PromptInput;
      id?: string | undefined;
    }): Turn;
  };
};

export type Agent<extended extends object = {}> = {
  readonly agentId: string;
  readonly key: string;
  readonly name: string;
  readonly sessionId: string;
  readonly type: string;
  readonly uid: string;
  extend<const extension extends object>(
    decorator: (agent: Agent<extended>) => extension,
  ): Agent<extended & extension>;

  dispose(): void;
} & extended;

declare const agentBackend: unique symbol;
export type DefaultAgent<B extends BackendName = never> = Agent<AgentActions<B>> & {
  readonly [agentBackend]?: B;
};


export type AgentLifecycle = {
  readonly agentId: string;
  readonly key: string;
  readonly name: string;
  readonly sessionId: string;
  readonly type: string;
  readonly uid: string;
  dispose(): void;
  events: {
    watch(options?: WatchEventsOptions): EventWatcher;
  };
  session: {

    shutdown(): Promise<void>;
  };
  turn: {
    prompt(options: {
      input: PromptInput;
      id?: string | undefined;
    }): LifecycleTurn;
  };
};

export type LifecycleTurn = Readonly<{
  readonly agent: Readonly<{
    agentId: string;
    key: string;
    name: string;
    sessionId: string;
    type: string;
    uid: string;
    dispose(): void;
  }>;
  accepted(): Promise<string | undefined>;
  result(): Promise<LifecycleTurnResult>;
  steer(options: { input: PromptInput; messageId?: string }): Promise<void>;

  withdrawSteer(options: { messageId: string }): Promise<boolean>;
  cancel(): Promise<void>;
  dispose(): void;
}>;

export type LifecycleTurnResult = Readonly<{
  finalMessage: string;

  usage(): Promise<TurnUsage | null>;
  dispose(): void;
}>;

export type Turn<agent extends Agent<object> = Agent<object>> = Readonly<{
  readonly agent: agent;





  accepted(): Promise<string | undefined>;




  result(): Promise<TurnResult>;
  steer(options: { input: PromptInput; messageId?: string }): Promise<void>;

  withdrawSteer(options: { messageId: string }): Promise<boolean>;
  cancel(): Promise<void>;

  dispose(): void;
}>;

declare const turnResultBrand: unique symbol;

export type TurnResult = Readonly<{
  readonly [turnResultBrand]: "NanocodexTurnResult";
  finalMessage: string;

  checkpoint(): Promise<SessionCheckpoint>;
  usage(): Promise<TurnUsage>;
  dispose(): void;
}>;

import type { NamedTool, ToolContext, ToolMap } from "nanocodex-tools";
export type {
  NamedTool,
  SubagentToolContext,
  Tool,
  ToolContext,
  ToolMap,
} from "nanocodex-tools";


export type ToolConfiguration<Extension = never> =
  | ToolMap
  | readonly (NamedTool | Extension)[]
  | import("./tools/Tools.mjs").Tools;

export type CodeEvaluatorEnvironment = {

  sessionId?: string;
  tools: Readonly<Record<string, (input: unknown) => Promise<unknown>>>;
  toolDefinitions: readonly Record<string, unknown>[];
  text(value: unknown): void;
  image(value: unknown, detail?: string): void;
  generatedImage(value: unknown): void;
  audio(value: unknown): void;
  notify(value: unknown): void;
  yield_control(): void;
  setTimeout(callback: () => void, delayMs?: number): number;
  clearTimeout(timerId?: number): void;
  store(key: string, value: unknown): void;
  load(key: string): unknown;
  exit(): never;
  require?: unknown;
  console?: Console;

  signal: AbortSignal;
};

export type CodeEvaluator = (
  source: string,
  environment: CodeEvaluatorEnvironment,
) => void | Promise<void>;


export type CodeEffectContext = Readonly<{
  sessionId: string;

  turnId?: string;

  operationId?: string;

  modelCallIndex?: number;

  parentCallId: string;
  callId: string;
  name: string;

  source: string;
  input: unknown;
}>;



export type CodeEffectReceipt = Readonly<{
  output: unknown;

  outputJsonRef?: "structured_result";
  structured_result: unknown;

  structuredResultRef?: "output";
  success: boolean;
  metadata: unknown;

  value: unknown;

  valueUndefined?: boolean;

  valueRef?: "output" | "structured_result";

  thrown: boolean;
  failure?: unknown;
}>;



export type CodeEffectJournal = Readonly<{






  observations?: Readonly<{
    register(context: CodeEffectContext, cellId: string): Promise<void>;
    record(sessionId: string, cellId: string, encodedObservation: string): Promise<void>;
    recover(sessionId: string, cellId: string): Promise<string | null>;
  }>;









  beginCell?(context: CodeEffectContext): Promise<readonly (readonly [string, unknown])[]>;
  commitStore?(context: CodeEffectContext, writes: readonly (readonly [string, unknown])[]): Promise<void>;
  begin(context: CodeEffectContext): Promise<
    | { status: "execute" }
    | { status: "replay"; receipt: CodeEffectReceipt }
    | { status: "unknown" }
  >;
  complete(context: CodeEffectContext, receipt: CodeEffectReceipt): Promise<void>;
}>;

declare const mcpPaymentBrand: unique symbol;
declare const lazyMcpPaymentBrand: unique symbol;


export type PaidMcpPayment = McpPayment & { readonly [mcpPaymentBrand]: true };


export type LazyPaidMcpPayment = { readonly [lazyMcpPaymentBrand]: true };

export type McpPayment = {

  methods: readonly unknown[];


  context?: unknown;

  onPaymentRequired?: ((challenge: unknown) => boolean | Promise<boolean>) | undefined;
  orderChallenges?: ((challenges: readonly unknown[]) => readonly unknown[] | Promise<readonly unknown[]>) | undefined;
  paymentPreferences?: unknown;
};

export type McpClient = {
  listTools(params?: { cursor?: string | undefined }, options?: Record<string, unknown>): Promise<{
    tools: readonly McpTool[];
    nextCursor?: string | undefined;
  }>;
  callTool(
    params: { name: string; arguments?: Record<string, unknown> | undefined },
    resultSchema?: unknown,
    options?: Record<string, unknown>,
  ): Promise<unknown>;
};

export type McpTool = {
  name: string;
  title?: string | undefined;
  description?: string | undefined;
  inputSchema?: Record<string, unknown> | undefined;
  annotations?: Readonly<Record<string, unknown>> & {
    readOnlyHint?: boolean | undefined;
  } | undefined;
};


export type McpPrivateResultPolicy = {



  beforeCall?: ((call: { name: string; arguments: Record<string, unknown> }, context: ToolContext | undefined) => void | { privateContext?: unknown; result?: unknown } | Promise<void | { privateContext?: unknown; result?: unknown }>) | undefined;



  transformResult: (call: { name: string; arguments: Record<string, unknown>; result: unknown; privateContext: unknown }, context: ToolContext | undefined) => unknown | Promise<unknown>;
};

export type McpServer = {

  url?: string | URL | undefined;

  client?: McpClient | undefined;
  description?: string | undefined;
  headers?: HeadersInit | undefined;
  fetch?: typeof globalThis.fetch | undefined;

  payment?: PaidMcpPayment | LazyPaidMcpPayment | undefined;



  privateResult?: McpPrivateResultPolicy | undefined;

  enabledTools?: readonly string[] | undefined;
  disabledTools?: readonly string[] | undefined;

  supportsParallelToolCalls?: boolean | undefined;

  parallelTools?: readonly string[] | undefined;

  isAvailable?: (() => boolean) | undefined;
  startupTimeoutMs?: number | undefined;
  timeoutMs?: number | undefined;
};

export type McpServers = Record<string, string | URL | McpServer>;


export type MppSession = {
  ws(endpoint: string | URL): Promise<MppWebSocket>;
  close?(): unknown | Promise<unknown>;
};

export type MppWebSocket = {
  readonly readyState: number;
  readonly bufferedAmount?: number | undefined;
  addEventListener(type: string, listener: (event: any) => void, options?: unknown): void;
  send(message: string): void;
  close(code?: number, reason?: string): void;
};
