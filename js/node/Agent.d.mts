import type {
  AgentLifecycle,
  AgentOptions,
  CodeEvaluator,
  CodeEffectJournal,
  DefaultAgent,
  DurabilityStore,
  McpServers,
  ToolConfiguration,
} from "../types.mjs";
import type { ResponsesTransport } from "./Transport.mjs";
import type { Tool as SubagentTool } from "../runtime/subagents.mjs";
import type { Workspace } from "./workspace.mjs";
import type { Tools } from "../tools/Tools.mjs";

export type Agent = DefaultAgent;

type ToolExposureOptions =
  | { mcp?: false | undefined; toolMode?: "code-only" | undefined }
  | { mcp: McpServers; toolMode?: "code-only" | undefined };


export function create(options: create.ClaudeOptions): Promise<DefaultAgent<"claude">>;
export function create(options: create.CodexOptions): Promise<DefaultAgent<"codex">>;
export function create(options: create.Options): Promise<create.ReturnType>;
export declare namespace create {

  type CodexOptions = AgentOptions & ToolExposureOptions & {
    codeEvaluator?: CodeEvaluator | undefined;

    codeEffectJournal?: CodeEffectJournal | undefined;

    filesystem?: Workspace | undefined;
    module?: unknown;
    transport: ResponsesTransport;
  } & (
    | {
      durability?: undefined;
      durabilityId?: undefined;
      tools?: ToolConfiguration<SubagentTool> | undefined;
    }
    | {
      durability: DurabilityStore;
      durabilityId: string;

      tools?: ToolConfiguration<SubagentTool> | undefined;
    }
  );

  type ClaudeOptions = import('../runtime/claude.mjs').Options & { harness: 'claude' };

  type Options = CodexOptions | ClaudeOptions;
  type ReturnType = Agent;
}
