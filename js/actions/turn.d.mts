import type {
  Agent,
  PromptInput,
  SessionCheckpoint,
  Turn,
  TurnResult,
  TurnUsage,
} from "../types.mjs";


export function prompt<const agent extends Agent<object>>(
  agent: agent,
  options: prompt.Options,
): prompt.ReturnType<agent>;
export declare namespace prompt {
  type Options = { input: PromptInput; id?: string | undefined };
  type ReturnType<agent extends Agent<object> = Agent<object>> = Turn<agent>;
}


export function accepted(turn: Turn): Promise<accepted.ReturnType>;
export declare namespace accepted {
  type ReturnType = string | undefined;
}


export function getResult(turn: Turn): Promise<getResult.ReturnType>;
export declare namespace getResult {
  type ReturnType = TurnResult;
}


export function getCheckpoint(result: TurnResult): Promise<getCheckpoint.ReturnType>;
export declare namespace getCheckpoint {
  type ReturnType = SessionCheckpoint;
}


export function getUsage(result: TurnResult): Promise<getUsage.ReturnType>;
export declare namespace getUsage {
  type ReturnType = TurnUsage;
}


export function steer(turn: Turn, options: steer.Options): Promise<void>;
export declare namespace steer {
  type Options = { input: PromptInput; messageId?: string };
  type ReturnType = void;
}


export function cancel(turn: Turn): Promise<void>;
export declare namespace cancel {
  type ReturnType = void;
}


export function withdrawSteer(turn: Turn, options: { messageId: string }): Promise<boolean>;
