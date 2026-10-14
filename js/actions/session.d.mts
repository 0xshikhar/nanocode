import type {
  Agent,
  AgentSessionContext,
  DefaultAgent,
  ForkOptions,
  BackendModels,
  BackendName,
  RealtimeTranscriptEntry,
  SessionCapabilities,
  SessionCheckpoint,
  SessionInfo,
  SessionPersistence,
  SideAnswer,
  TranscriptMessage,
  ServiceTier,
  Thinking,
} from "../types.mjs";


export function info(agent: Agent<object>): SessionInfo;


export function capabilities(agent: Agent<object>): SessionCapabilities;


export function persistence(agent: Agent<object>): SessionPersistence | null;


export function checkpoint(agent: Agent<object>): Promise<SessionCheckpoint>;


export function cancel(agent: Agent<object>): Promise<void>;


export function appendDeveloperMessage(
  agent: Agent<object>,
  text: string,
): Promise<AgentSessionContext>;


export function startRealtimeConversation(agent: Agent<object>): Promise<AgentSessionContext>;


export function endRealtimeConversation(agent: Agent<object>): Promise<AgentSessionContext>;


export function realtimeDelegation(
  agent: Agent<object>,
  input: string,
  transcript?: readonly RealtimeTranscriptEntry[],
): Promise<string>;


export function realtimeTailDelegation(
  agent: Agent<object>,
  transcript: readonly RealtimeTranscriptEntry[],
): Promise<string | undefined>;


export function compact(agent: Agent<object>): Promise<void>;


export function context(agent: Agent<object>): Promise<AgentSessionContext>;

export function sideQuestion(agent: Agent<object>, question: string): Promise<SideAnswer>;

export function transcript(agent: Agent<object>): Promise<readonly TranscriptMessage[]>;





export function fork<B extends BackendName>(agent: DefaultAgent<B>, options?: fork.Options): Promise<DefaultAgent<B>>;
export function fork(agent: Agent<object>, options?: fork.Options): Promise<fork.ReturnType>;
export declare namespace fork {
  type Options = ForkOptions;
  type ReturnType = DefaultAgent;
}


export function spawn<B extends BackendName>(agent: DefaultAgent<B>): Promise<DefaultAgent<B>>;
export function spawn(agent: Agent<object>): Promise<spawn.ReturnType>;
export declare namespace spawn {
  type ReturnType = DefaultAgent;
}


export function setThinking(agent: Agent<object>, thinking: Thinking): Promise<void>;


export function setModel<B extends BackendName>(agent: DefaultAgent<B>, model: BackendModels[NoInfer<B>]): Promise<void>;


export function setFastMode(agent: Agent<object>, enabled: boolean): Promise<void>;






export function setServiceTier(agent: Agent<object>, serviceTier: ServiceTier): Promise<void>;


export function shutdown(agent: Agent<object>): Promise<void>;
