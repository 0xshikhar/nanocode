import type { Workspace, WorkspaceBackend, WorkspaceEntry } from "../runtime/workspace.mjs";

export { tools } from "../runtime/workspace.mjs";

type OpfsStorage = {
  getDirectory(): Promise<FileSystemDirectoryHandle>;
};

export function open(options?: {
  name?: string | undefined;
  root?: string | undefined;

  storage?: OpfsStorage | undefined;
}): Promise<Workspace>;

export type { Workspace, WorkspaceBackend, WorkspaceEntry };
