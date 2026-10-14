import type { NamedTool } from "nanocodex-tools";
export type ComputerToolDefinition = Readonly<{
  name: string;
  description?: string;
  inputSchema: Record<string, unknown>;
  outputSchema?: Record<string, unknown>;
  annotations?: Readonly<Record<string, unknown>>;
  _meta?: Readonly<Record<string, unknown>>;
  [key: string]: unknown;
}>;
export type ComputerOptions = {
  executable: string;
  args?: readonly string[];
  environment?: Record<string, string>;

  definitions?: readonly ComputerToolDefinition[];
};
export type ComputerProviderTool = NamedTool & Readonly<{ providerDefinition: ComputerToolDefinition }>;
export type ComputerAttachment = Readonly<{

  definitions: readonly ComputerToolDefinition[];

  tools: readonly ComputerProviderTool[];

  tool(name: string): ComputerProviderTool | undefined;

  endTurn(sessionId: string, turnId: string, hookEventName?: "Stop" | "Interrupt" | "SubagentStop"): Promise<void>;
  close(): Promise<void>;
}>;
export function createComputerTools(options: ComputerOptions): ComputerAttachment;
export function connectComputerTools(options?: ComputerOptions): Promise<ComputerAttachment>;

export { connectComputerTools as connect };
export const CUA_JS_NAME: "mcp__cua_repl__js";
export const CUA_RESET_NAME: "mcp__cua_repl__js_reset";

export function installedConfig(): ComputerOptions;
