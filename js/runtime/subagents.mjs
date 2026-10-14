import {
  closeSubagent,
  interruptSubagent,
  listSubagents,
  sendSubagentMessage,
  setSubagentSpawnPolicy,
  spawnSubagent,
  spawnSubagents,
  waitSubagents,
} from "../internal.mjs";
import {
  defaultSubagentMaxConcurrency,
  subagentsBrand,
} from "./tool-configuration.mjs";
import { spawnPolicyFromWire, spawnPolicyWire } from "./spawn-policy.mjs";

export function create(options = {}) {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("Subagents.create options must be an object");
  }
  const maxConcurrency = options.maxConcurrency ?? defaultSubagentMaxConcurrency;
  if (maxConcurrency !== undefined && (!Number.isSafeInteger(maxConcurrency) || maxConcurrency < 1)) {
    throw new TypeError("subagents maxConcurrency must be a positive safe integer");
  }
  const continuation = options.continuation;
  if (continuation !== undefined && typeof continuation !== "boolean") {
    throw new TypeError("subagents continuation must be a boolean");
  }
  const spawnPolicy = options.spawnPolicy === undefined ? undefined : spawnPolicyWire(options.spawnPolicy);
  return Object.freeze([Object.freeze({
    [subagentsBrand]: Object.freeze({ maxConcurrency, continuation, spawnPolicy }),
  })]);
}

export async function setSpawnPolicy(agent, policy) {
  return spawnPolicyFromWire(setSubagentSpawnPolicy(agent, spawnPolicyWire(policy)));
}

export function spawn(agent, options) {
  return spawnSubagent(agent, options);
}

export function spawnMany(agent, options) {
  return spawnSubagents(agent, options);
}

export function wait(agent, options) {
  return waitSubagents(agent, options);
}

export function list(agent, options) {
  return listSubagents(agent, options);
}

export function send(agent, options) {
  return sendSubagentMessage(agent, options);
}

export function interrupt(agent, agentId) {
  return interruptSubagent(agent, agentId);
}

export function close(agent, agentId) {
  return closeSubagent(agent, agentId);
}
