import type { Workspace } from "./types.mjs";

export type RepositoryWorkspaceFetch = (
  url: string,
  options?: Readonly<{
    method?: string | undefined;
    headers?: Headers | Record<string, string> | undefined;
    body?: Uint8Array | undefined;
    signal?: AbortSignal | undefined;
  }>,
) => Promise<Response | Readonly<{
  status: number;
  statusText?: string | undefined;
  headers?: Headers | Record<string, string> | undefined;
  body?: Uint8Array | AsyncIterable<Uint8Array> | undefined;
  url?: string | undefined;
  arrayBuffer?(): Promise<ArrayBuffer>;
}>>;

export type RepositoryWorkspaceRemote = Readonly<{
  name: string;
  url: string;
  branch: string;
}>;

export type RepositoryWorkspaceDescriptor = Readonly<{
  branch: string;
  directory: string;
  gitDirectory: string;
  head: string;
  markerPath: string;
  seedUrl: string;
  writableRemote?: RepositoryWorkspaceRemote | undefined;
}>;






export function materializeRepositoryWorkspace(options: Readonly<{
  workspace: Workspace;
  fetch: RepositoryWorkspaceFetch;

  seedUrl: string;
  head: string;
  branch?: string | undefined;

  directory?: string | undefined;
  writableRemote?: Readonly<{
    url: string;
    name?: string | undefined;
    branch?: string | undefined;
  }> | undefined;
}>): Promise<RepositoryWorkspaceDescriptor>;
