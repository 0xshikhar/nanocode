import { tools as workspaceTools } from "./workspace.mjs";
import { claudeWorkspaceTools } from "./claude-workspace-tools.mjs";
import { ToolRouter, toolRouterBrand, toolRouterRuntime, toolRuntimeLifecycle, toolMapSource } from "./tool-router.mjs";
import { EXEC_COMMAND_PARAMETERS, WRITE_STDIN_PARAMETERS } from "nanocodex-tools/execution-contract";

const MAX = Number.MAX_SAFE_INTEGER;
const RESULT = Symbol.for("nanocodex.toolResult");
const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const failure = (message, code = "runner_unavailable") => Object.assign(new Error(message), { code });

export function createRunnerPool(platform, workspace, notify, store) {
  const routes = new Map();
  const listeners = new Set();
  const sockets = new Set();
  const observers = new Set();
  let closed = false;
  const stateOwner = globalThis.crypto.randomUUID();
  const states = new Map();
  const locks = new Map();
  async function stateFor(context) {
    const id = (context?.sessionId ?? "javascript") + ":runner-routes";
    if (!states.has(id)) states.set(id, (async () => {
      if (!store) return { id, data: { mounts: [], sessions: {} } };
      const acquired = await store.acquire(id, { ownerId: stateOwner });
      return { id, fence: acquired.fence, revision: acquired.revision, data: acquired.payload ? JSON.parse(acquired.payload) : { mounts: [], sessions: {} } };
    })());
    return states.get(id);
  }
  async function update(state, mutate) {
    const previous = locks.get(state.id) ?? Promise.resolve();
    const current = previous.then(async () => {
      const next = structuredClone(state.data);
      mutate(next);
      if (store) {
        const result = await store.replace(state.id, { ownerId: stateOwner, fence: state.fence, expectedRevision: state.revision, payload: JSON.stringify(next), records: [] });
        if (result.status !== "replaced") throw failure("runner routing persistence failed: " + result.status);
        state.revision = result.revision;
      }
      state.data = next;
    });
    locks.set(state.id, current);
    try { await current; } finally { if (locks.get(state.id) === current) locks.delete(state.id); }
  }
  const emit = event => { for (const observer of observers) { try { observer(event); } catch {} } };
  const announce = () => notify(mountMessage());
  const mountMessage = () => "Runner mounts: " + ([...routes.values()].map(route => `/${route.name} -> ${route.workspace} (${route.status}, ${route.mode})`).join("; ") || "none") + ". Select a runner using workdir or an absolute file path under its mount. Unavailable mounts must not run locally.";
  function options(value) {
    if (!value || typeof value !== "object") throw new TypeError("runner options are required");
    if (value.name !== undefined && !NAME.test(value.name)) throw new TypeError("invalid runner name");
    if (value.mode !== undefined && !["alongside", "replace"].includes(value.mode)) throw new TypeError("runner mode must be alongside or replace");
    if (value.callTimeoutMs !== undefined && (!Number.isSafeInteger(value.callTimeoutMs) || value.callTimeoutMs <= 0)) throw new TypeError("callTimeoutMs must be a positive safe integer");
    return { ...value, mode: value.mode ?? "alongside" };
  }
  function accept(socket, config) {
    sockets.add(socket);
    return new Promise((resolve, reject) => {
      let route;
      const pending = new Map();
      const send = frame => { if (socket.readyState !== 1) throw failure("runner disconnected"); socket.send(JSON.stringify(frame)); };
      const end = error => {
        sockets.delete(socket);
        if (!route) reject(error);
        else {
          route.status = "disconnected";
          for (const [id, call] of pending) call.finish(failure(`runner ${route.name} disconnected after dispatch; outcome unknown (${id})`, "runner_ambiguous"));
          emit({ type: "disconnected", runner: route.handle, message: error.message });
          void announce().catch(error => emit({ type: "rejected", message: error.message }));
        }
      };
      socket.addEventListener("error", () => { if (!route) reject(failure("runner WebSocket failed")); });
      socket.addEventListener("close", () => end(failure("runner WebSocket closed")));
      socket.addEventListener("message", event => {
        try {
          const frame = JSON.parse(typeof event.data === "string" ? event.data : event.data.toString());
          if (!route) {
            if (frame.type !== "catalog" || !Array.isArray(frame.tools) || typeof frame.runtime_id !== "string") throw failure("invalid runner catalog");
            const name = config.name ?? frame.machines?.[0]?.id;
            const root = frame.machines?.[0]?.workspace;
            if (!NAME.test(name ?? "") || typeof root !== "string" || !root.startsWith("/")) throw failure("runner catalog requires a machine id and canonical workspace");
            if (routes.get(name)?.status === "connected") throw failure("duplicate active runner: " + name);
            const tools = new Map();
            for (const entry of frame.tools) {
              if (typeof entry.definition?.name !== "string" || !Number.isSafeInteger(entry.timeout_ms) || entry.timeout_ms <= 0) throw failure("invalid runner tool catalog");
              if (tools.has(entry.definition.name)) throw failure("duplicate runner catalog tool");
              tools.set(entry.definition.name, entry);
            }
            route = { name, workspace: root.replace(/\/$/, ""), mode: config.mode, status: "connected", tools, socket, runtimeId: frame.runtime_id };
            route.invoke = (name, input, context = {}) => {
              const spec = tools.get(name);
              if (!spec || route.status !== "connected") return Promise.reject(failure(`runner ${route.name}: ${name} unavailable`));
              const signal = context.signal;
              signal?.throwIfAborted();
              const id = globalThis.crypto.randomUUID();
              const timeout = config.callTimeoutMs ?? spec.timeout_ms;
              const deadline = timeout === MAX ? MAX : Date.now() + timeout;
              if (!Number.isSafeInteger(deadline)) return Promise.reject(failure("runner deadline exceeds protocol integer range"));
              return new Promise((resolveCall, rejectCall) => {
                let timer;
                const abort = () => { try { send({ type: "cancel", call_id: id }); } catch {} finish(failure("runner call cancelled after dispatch; outcome unknown", "runner_ambiguous")); };
                const finish = (error, value) => {
                  if (!pending.delete(id)) return;
                  clearTimeout(timer);
                  signal?.removeEventListener("abort", abort);
                  if (error) rejectCall(error); else resolveCall(value);
                };
                pending.set(id, { finish });
                signal?.addEventListener("abort", abort, { once: true });
                try {
                  send({ type: "call", session_id: context.sessionId ?? "javascript", ...(context.turnId ? { turn_id: context.turnId } : {}), call_id: id, model: context.model ?? "unknown", name: spec.remote_name ?? spec.definition.name, input, output_token_budget: MAX, output_byte_budget: MAX, deadline_at: deadline });
                  if (timeout !== MAX) timer = setTimeout(abort, timeout);
                  if (signal?.aborted) abort();
                } catch (error) { finish(error); }
              });
            };
            route.handle = Object.freeze({ name, workspace: route.workspace, mode: route.mode, get status() { return route.status; }, disconnect: async () => { if (socket.readyState === 3) return; await new Promise(resolve => { socket.addEventListener("close", resolve, { once: true }); socket.close(); }); } });
            routes.set(name, route);
            send({ type: "ready" });
            emit({ type: "connected", runner: route.handle });
            void announce().catch(error => emit({ type: "rejected", message: error.message }));
            resolve(route.handle);
          } else if (frame.type === "result") {
            send({ type: "ack", call_id: frame.call_id });
            const outcome = frame.outcome;
            if (outcome?.status === "completed") {
              const output = outcome.output;
              pending.get(frame.call_id)?.finish(null, { [RESULT]: true, output: output.output, structuredResult: output.structured_result, value: output.structured_result ?? output.output, metadata: output.metadata, success: output.success });
            } else pending.get(frame.call_id)?.finish(failure(outcome?.message ?? "runner outcome unknown", outcome?.status === "ambiguous" ? "runner_ambiguous" : "runner_unavailable"));
          } else if (frame.type === "status" && frame.state !== "running") pending.get(frame.call_id)?.finish(failure("runner call outcome unknown", "runner_ambiguous"));
          else if (frame.type === "drain") { send({ type: "draining" }); socket.close(); }
        } catch (error) { reject(error); socket.close(1008, "runner rejected"); }
      });
    });
  }
  async function connectRunner(value) {
    if (closed) throw failure("runner pool closed");
    const config = options(value);
    const url = new URL(config.url);
    if (!["ws:", "wss:"].includes(url.protocol) || url.username || url.password) throw new TypeError("runner url must be ws or wss without credentials");
    if (!platform.connectRunnerSocket) throw failure("runner WebSocket transport unavailable", "unsupported_capability");
    return accept(await platform.connectRunnerSocket(config), config);
  }
  async function listenForRunners(value = {}) {
    if (closed) throw failure("runner pool closed");
    if (!platform.listenRunnerSockets) throw failure("incoming runners require Node", "unsupported_capability");
    const config = options(value);
    const server = await platform.listenRunnerSockets(config, socket => { void accept(socket, config).catch(error => emit({ type: "rejected", message: error.message })); });
    listeners.add(server);
    return Object.freeze({ address: server.address, url: server.url, token: server.token, stop: async () => { listeners.delete(server); await server.stop(); } });
  }
  function select(input, state) {
    const path = input?.workdir ?? input?.file_path ?? input?.path;
    if (typeof path === "string" && path.startsWith("/")) {
      const name = path.split("/")[1];
      const route = routes.get(name);
      if (route) return route;
      if (path !== workspace && !path.startsWith(workspace + "/")) throw failure("runner mount unavailable: /" + name);
    }
    if (state.data.replacement) {
      const route = routes.get(state.data.replacement);
      if (!route) throw failure("runner mount unavailable: /" + state.data.replacement);
      return route;
    }
    const replacements = [...routes.values()].filter(route => route.mode === "replace");
    if (replacements.length > 1) throw failure("multiple replace runners require an explicit mount");
    return replacements[0];
  }
  function wrapTools(configuration, extra = {}) {
    if (configuration?.[toolRouterBrand]) {
      const original = configuration[toolRouterRuntime] ?? configuration;
      const router = new ToolRouter();
      router.addSource({ id: "runner-routing", kind: "cloud", definitions: () => original.definitions(), resolve(name) {
        const tool = original.resolve(name);
        if (!tool) return undefined;
        return wrapTools({ [name]: tool })[name];
      } });
      const missing = Object.fromEntries(Object.entries(extra).filter(([name]) => !original.resolve(name)));
      if (Object.keys(missing).length) router.addSource(toolMapSource("runner-process", wrapTools(missing), { kind: "local" }));
      return { [toolRouterBrand]: true, [toolRouterRuntime]: router, [toolRuntimeLifecycle]: configuration[toolRuntimeLifecycle] };
    }
    const map = Array.isArray(configuration) ? Object.fromEntries(configuration.filter(tool => typeof tool.name === "string").map(({ name, ...tool }) => [name, tool])) : configuration;
    if (!map || typeof map !== "object") throw new TypeError("runner routing requires a tool array or map");
    const wrapped = Object.fromEntries(Object.entries(map).map(([name, tool]) => [name, { ...tool, async handler(input, context) {
      const state = await stateFor(context);
      let route;
      let session;
      if (name === "write_stdin") {
        session = state.data.sessions[input.session_id];
        if (!session) throw failure("process session route unavailable: " + input.session_id);
        route = session.name === null ? undefined : routes.get(session.name);
        if (session.name !== null && (!route || route.runtimeId !== session.runtimeId)) throw failure("process session runner unavailable: " + session.name);
      } else route = select(input, state);
      if (route) await update(state, data => {
        if (!data.mounts.includes(route.name)) data.mounts.push(route.name);
        if (route.mode === "replace" && !data.replacement) data.replacement = route.name;
      });
      const retainSession = async result => {
        const value = result?.[RESULT] ? result.structuredResult : result;
        if (Number.isSafeInteger(value?.session_id)) {
          const remoteId = value.session_id;
          let virtual = input.session_id;
          if (name !== "write_stdin") {
            do { const bytes = new Uint32Array(1); globalThis.crypto.getRandomValues(bytes); virtual = bytes[0]; } while (!virtual || state.data.sessions[virtual]);
            await update(state, data => { data.sessions[virtual] = { name: route?.name ?? null, runtimeId: route?.runtimeId ?? null, remoteId }; });
          }
          const translatedValue = { ...value, session_id: virtual };
          if (result?.[RESULT]) return { ...result, structuredResult: translatedValue, value: translatedValue, output: typeof result.output === "string" ? result.output.replace(/Process running with session ID \d+/, "Process running with session ID " + virtual) : result.output };
          return translatedValue;
        }
        return result;
      };
      if (!route) return retainSession(await tool.handler(session ? { ...input, session_id: session.remoteId } : input, context));
      const translated = session ? { ...input, session_id: session.remoteId } : { ...input };
      for (const key of ["workdir", "file_path", "path"]) if (typeof translated[key] === "string" && (translated[key] === "/" + route.name || translated[key].startsWith("/" + route.name + "/"))) {
        const suffix = translated[key].slice(route.name.length + 1);
        if (suffix.split("/").includes("..")) throw failure("runner mount path escapes workspace");
        translated[key] = route.workspace + suffix;
      }
      if (name === "exec_command" && translated.workdir === undefined) translated.workdir = route.workspace;
      let result;
      if (route.tools.has(name)) result = await route.invoke(name, translated, context);
      else if (["read_file", "write_file", "list_files", "remove_file", "make_directory", "Read", "Write", "Edit", "Glob", "Grep"].includes(name)) {
        const quote = value => "'" + value.replace(/'/g, "'\"'\"'") + "'";
        const canonical = file => {
          const path = file.startsWith("/") ? file : route.workspace + "/" + file;
          if ((path !== route.workspace && !path.startsWith(route.workspace + "/")) || path.split("/").includes("..")) throw failure("remote file path escapes workspace");
          return path;
        };
        const execute = async cmd => {
          const execution = await route.invoke("exec_command", { cmd, workdir: route.workspace, login: false, yield_time_ms: 10000 }, context);
          if (!execution.success || execution.structuredResult?.exit_code !== 0) throw failure(execution.output);
          return execution.structuredResult.output;
        };
        const remoteWorkspace = {
          root: route.workspace,
          async readFile(file) {
            const encoded = (await execute("base64 < " + quote(canonical(file)))).replace(/\s/g, "");
            return Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
          },
          async writeFile(file, bytes) {
            const path = canonical(file);
            const encoded = btoa(Array.from(bytes, byte => String.fromCharCode(byte)).join(""));
            await execute("mkdir -p -- " + quote(path.slice(0, path.lastIndexOf("/"))) + " && printf %s " + quote(encoded) + " | base64 -d > " + quote(path));
          },
          async list(file = ".", options = {}) {
            const path = canonical(file);
            const prefix = "find " + quote(path) + " -mindepth 1 " + (options.recursive ? "" : "-maxdepth 1 ");
            const paths = (await execute(prefix + "-print0")).split("\0").filter(Boolean);
            if (options.maxEntries !== undefined && paths.length > options.maxEntries) throw failure("remote listing exceeds maxEntries");
            const directories = new Set((await execute(prefix + "-type d -print0")).split("\0").filter(Boolean));
            return paths.map(path => ({ path, kind: directories.has(path) ? "directory" : "file" }));
          },
          async remove(file, options = {}) { await execute("rm " + (options.recursive ? "-r " : "") + "-- " + quote(canonical(file))); },
          async mkdir(file) { await execute("mkdir -p -- " + quote(canonical(file))); },
        };
        const remoteTools = /^[A-Z]/.test(name) ? Object.fromEntries(claudeWorkspaceTools(remoteWorkspace).map(tool => [tool.name, tool])) : workspaceTools(remoteWorkspace);
        if (!remoteTools[name]) throw failure(`runner ${route.name}: ${name} unavailable`);
        result = await remoteTools[name].handler(translated, context);
      } else throw failure(`runner ${route.name}: ${name} unavailable`);
      return retainSession(result);
    } }]));
    return Array.isArray(configuration) ? configuration.map(tool => typeof tool.name === "string" ? { name: tool.name, ...wrapped[tool.name] } : tool) : wrapped;
  }
  return Object.freeze({ connectRunner, listenForRunners, wrapTools, mountMessage, onRunnerEvent(observer) { observers.add(observer); return () => observers.delete(observer); }, listRunners: () => [...routes.values()].map(route => route.handle), async disconnectRunner(name) { const route = routes.get(name); if (!route) throw failure("unknown runner: " + name); await route.handle.disconnect(); }, async stopRunnerListeners() { const current = [...listeners]; listeners.clear(); await Promise.all(current.map(server => server.stop())); }, async close() { closed = true; await this.stopRunnerListeners(); for (const socket of sockets) socket.close(); } });
}

export function unavailableProcessTools() {
  const handler = () => { throw failure("local process execution unavailable"); };
  return { exec_command: { description: "Run a shell command. Select a runner mount using workdir.", parameters: EXEC_COMMAND_PARAMETERS, handler }, write_stdin: { description: "Write to a shell session.", parameters: WRITE_STDIN_PARAMETERS, handler } };
}
