const fields = Object.freeze({
  models: "models",
  modelLadder: "model_ladder",
  minThinking: "min_thinking",
  maxThinking: "max_thinking",
  boundedByParentThinking: "bounded_by_parent_thinking",
});

export function spawnPolicyWire(policy) {
  if (!policy || typeof policy !== "object" || Array.isArray(policy)) {
    throw new TypeError("subagents spawnPolicy must be an object");
  }
  const wire = {};
  for (const [key, value] of Object.entries(policy)) {
    if (!Object.hasOwn(fields, key)) throw new TypeError(`unknown subagents spawnPolicy option: ${key}`);
    if (value !== undefined) wire[fields[key]] = value;
  }
  return Object.freeze(wire);
}

export function spawnPolicyFromWire(wire) {
  const policy = {};
  for (const [key, name] of Object.entries(fields)) {
    if (Object.hasOwn(wire, name)) policy[key] = wire[name];
  }
  return Object.freeze(policy);
}
