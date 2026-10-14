import type { NamedTool } from "./types.mjs";

export type DatasetOptions = Readonly<{

  fetch?: typeof globalThis.fetch | undefined;
}>;


export function dataset(options?: DatasetOptions): NamedTool;
