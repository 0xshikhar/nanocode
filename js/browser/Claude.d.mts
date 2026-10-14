import type { Agent, Options } from '../runtime/claude.mjs';
export type { Agent, Auth, Options, NativeToolResult, Tool, ToolContent, ToolResult } from '../runtime/claude.mjs';

export function create(options: Options): Promise<Agent>;
