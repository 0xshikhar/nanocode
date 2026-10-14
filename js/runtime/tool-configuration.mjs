export * from "nanocodex-tools/runtime/tool-configuration";
import { resolveTools as resolveUpstreamTools } from "nanocodex-tools/runtime/tool-configuration";

export function resolveTools(configuration, options) {
  const resolved = resolveUpstreamTools(configuration, options);
  const extension = configuration?.[Symbol.for("nanocodex.runnerSubagents")];
  if (!extension) return resolved;
  return { ...resolved, subagents: { ...(extension.maxConcurrency === undefined ? {} : { max_concurrency: extension.maxConcurrency }), ...(extension.continuation === undefined ? {} : { continuation: extension.continuation }), ...(extension.spawnPolicy === undefined ? {} : { spawn_policy: extension.spawnPolicy }) } };
}
