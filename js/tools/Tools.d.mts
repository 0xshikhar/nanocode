import type { McpServers, NamedTool, ToolMap } from "../types.mjs";
import type { Workspace } from "../runtime/workspace.mjs";
import type { HostedMachine } from "./hostedCatalog.mjs";

declare const toolsBrand: unique symbol;

export type AttachmentSocket = {
  readonly readyState: number;
  send(data: string): void;
  close(code?: number, reason?: string): void;
  addEventListener?(type: string, listener: (event: any) => void, options?: unknown): void;
  on?(type: string, listener: (...args: any[]) => void): void;
};

export type AttachmentTransport = Readonly<{
  connect(target: string | URL): AttachmentSocket | Promise<AttachmentSocket>;
}>;

export type AttachmentTarget = string | URL | Readonly<{
  endpoint: string | URL;
  transport: AttachmentTransport;
}>;

export type AttachmentClient = Readonly<{
  readonly connected: boolean;
  closed(): Promise<void>;
  close(): Promise<void>;
}>;

export type Tools = Readonly<{
  readonly [toolsBrand]: true;
  attach(target: AttachmentTarget): Readonly<{
    connect(): Promise<AttachmentClient>;
    closed(): Promise<void>;
    close(): Promise<void>;
  }>;
  close(): Promise<void>;
}>;






export function createTools(options?: {
  tools?: ToolMap | readonly NamedTool[];

  attachmentId?: string;

  machines?: readonly HostedMachine[];

  workspace?: Workspace;
  workspaceOptions?: { maxEntries?: number; maxReadBytes?: number; maxWriteBytes?: number };
  mcp?: McpServers | false;
  mcpOptions?: Readonly<Record<string, unknown>>;
}): Promise<Tools>;
