const modelAliases = new Map([
  ["kimi", "kimi"], ["kimi-k3", "kimi"], ["mimo", "mimo"], ["mimo-v2.6-pro", "mimo"],
  ['sol', 'sol'], ['gpt-6.1-sol', 'sol'],
  ['luna', 'luna'], ['gpt-6-luna', 'luna'],
  ['astra', 'astra'], ['gpt-6-astra', 'astra'],
  ...[['opus','claude-opus-5-5'], ['sonnet','claude-sonnet-5-5'], ['haiku','claude-haiku-5-5'], ['fable','claude-fable-5-1'], ...['claude-opus-5-5','claude-sonnet-5-5','claude-haiku-5-5','claude-fable-5-1','claude-opus-4-6','claude-sonnet-4-6','claude-haiku-4-5'].map(id => [id,id])],
  ['glm-5.3', 'glm-5.3'], ['glm53', 'glm-5.3'], ['@cf/zai-org/glm-5.3', 'glm-5.3'],
]);
const thinkingLevels = new Set(['none', 'low', 'medium', 'high', 'xhigh', 'max']);
function canonicalModel(model) {
  const canonical = modelAliases.get(model);
  if (!canonical) throw new TypeError('subagent router returned an invalid model');
  return canonical;
}
function synchronous(value, operation) {
  if (value && typeof value.then === 'function') {

    Promise.resolve(value).catch(() => {});
    throw new TypeError(`subagent route ${operation} must be synchronous`);
  }
  return value;
}


export function createSubagentRouting({ resolve, authorize, load, save }) {
  for (const [name, callback] of Object.entries({ resolve, authorize, load, save })) {
    if (typeof callback !== "function") throw new TypeError(`subagent routing ${name} must be a function`);
  }
  const pending = new Map();
  return Object.freeze({
    async resolve(request) {


      request = Object.freeze({ ...request,
        ...(request.model === undefined ? {} : { model: canonicalModel(request.model) }) });
      if (request.harness !== undefined && !['codex','claude'].includes(request.harness)) throw new TypeError('invalid harness family');
      if (request.harness && request.model && request.harness !== (request.model.startsWith('claude-') ? 'claude' : 'codex')) throw new TypeError('model does not belong to selected harness');
      const authority = await authorize(request.parentSessionId, request.hostContextRef);
      const choice = await resolve(request, authority);
      if (!choice || typeof choice.model !== "string" || typeof choice.provider !== "string"
        || typeof choice.thinking !== "string") throw new TypeError("subagent router returned an invalid choice");
      const model = canonicalModel(choice.model);
      const harness = choice.harness ?? (model.startsWith('claude-') ? 'claude' : 'codex');
      if (!['codex','claude'].includes(harness) || harness !== (model.startsWith('claude-') ? 'claude' : 'codex') || (request.harness !== undefined && request.harness !== harness)) throw new TypeError('subagent route conflicts with harness family');
      if (!choice.provider.trim() || !thinkingLevels.has(choice.thinking)
        || (model === 'sol' && choice.thinking === 'none')
        || (choice.providerModel !== undefined && (typeof choice.providerModel !== 'string' || !choice.providerModel.trim()))) {
        throw new TypeError('subagent router returned an invalid choice');
      }
      if (request.model !== undefined && request.model !== model) {
        throw new Error("subagent router cannot replace an explicit model override");
      }
      if (request.thinking !== undefined && request.thinking !== choice.thinking) {
        throw new Error("subagent router cannot replace an explicit thinking override");
      }


      const route = Object.freeze({ provider: choice.provider, model,
        thinking: choice.thinking, ...((choice.harness !== undefined || harness === 'claude') ? { harness } : {}), ...(choice.providerModel === undefined ? {} : { providerModel: choice.providerModel }) });
      const routeId = crypto.randomUUID();
      pending.set(routeId, { route, parentSessionId: request.parentSessionId,
        hostContextRef: request.hostContextRef });
      return { model: route.model, thinking: route.thinking, routeId, ...(route.harness === undefined ? {} : { harness: route.harness }) };
    },
    bind(request) {
      const prepared = pending.get(request.routeId);
      if (!prepared || prepared.parentSessionId !== request.parentSessionId
        || prepared.hostContextRef !== request.hostContextRef) {
        throw new Error("subagent route is not owned by this parent authorization");
      }
      const existing = synchronous(load(request.sessionId), 'load');
      if (existing !== undefined && existing !== null) {
        throw new Error("subagent route is already pinned");
      }


      synchronous(save(request.sessionId, prepared.route), 'save');
      pending.delete(request.routeId);
    },
    route(sessionId) {
      const retained = synchronous(load(sessionId), 'load');
      if (!retained) throw new Error("subagent route is missing; refusing to inherit parent transport");
      return retained;
    },
  });
}
