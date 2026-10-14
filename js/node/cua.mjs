import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { namedTool } from "nanocodex-tools/named-tool";
import { toolResult } from "nanocodex-tools/runtime/code-runtime";
const interruptedGuidance = "Upstream/native input may still be running and effects are uncertain; do not replay uncertain input. Call cua_repl.js_reset, then inspect the surface before continuing. Reset does not prove earlier input stopped.";

export function installedConfig() {
  if (process.platform !== "darwin") throw new Error("Default Sky CUA requires macOS; supply an explicit cua connection");
  const base = "/Applications/ChatGPT.app/Contents/Resources/cua_node";
  const home = process.env.CODEX_HOME || join(homedir(), ".codex");
  const executable = join(base, "bin/node_repl");
  const service = join(home, "computer-use/Codex Computer Use.app");
  if (!existsSync(executable) || !existsSync(service)) throw new Error(`Default Sky CUA installation missing: expected ${executable} and ${service}`);
  return { executable, environment: {
    NODE_REPL_NODE_PATH: join(base, "bin/node"),
    NODE_REPL_NODE_MODULE_DIRS: join(base, "lib/node_modules"),
    NODE_REPL_TRUSTED_CODE_PATHS: join(base, "lib/node_modules"),
    NODE_REPL_TRUSTED_SERVICES: JSON.stringify({ sky: "@oai/sky/service" }),
    SKY_CUA_SERVICE_PATH: service,
    CODEX_HOME: home,
  } };
}
export async function connectComputerTools(options = installedConfig()) {
  if (!options?.executable) throw new TypeError("A trusted CUA executable is required");
  const providerProcess = new ComputerProcess(options.executable, options.args ?? [], options.environment ?? {});
  try {
    const definitions = await providerProcess.start();
    return createComputerTools({ ...options, definitions });
  } finally { providerProcess.close(); }
}


export function createComputerTools(options) {
  const { executable, args = [], environment = {}, definitions } = options;
  if (!executable) throw new TypeError("A trusted CUA executable is required");
  if (definitions === undefined) throw new TypeError("MCP providers require connectComputerTools discovery or a trusted catalog");
  const catalog = validateCatalog(definitions);
  const sessions = new Map();
  let disposed = false;
  const releaseSession = id => {
    const session = sessions.get(id);
    sessions.delete(id);
    session?.lifetime.abort(new Error("CUA conversation was released"));
    session?.process?.close();
  };
  const close = async () => { disposed = true; for (const id of sessions.keys()) releaseSession(id); };
  const invoke = (name, input, context) => {
    const id = context.sessionId;
    if (!id) throw new Error("CUA requires a conversation identity");
    if (disposed) throw new Error("CUA attachment is closed");
    if (!sessions.has(id)) {
      sessions.set(id, {
        lifetime: new AbortController(), process: undefined, interrupted: false,
        tail: Promise.resolve(), turns: new Map(),
      });
    }
    const session = sessions.get(id);


    const signal = AbortSignal.any([session.lifetime.signal, ...(context.signal ? [context.signal] : [])]);
    let dispatched = false;
    const cancellationReason = () => dispatched
      ? new Error(`CUA call cancelled after dispatch; native effects may continue and completion is unknown. ${interruptedGuidance}`, { cause: signal.reason })
      : signal.reason;
    const run = async () => {
      signal.throwIfAborted();
      if (disposed) throw new Error("CUA attachment is closed");
      if (sessions.get(id) !== session) throw new Error("CUA conversation was released");
      if (session.interrupted && name !== "js_reset") throw new Error(`CUA session interrupted. ${interruptedGuidance}`);
      const abort = () => { session.process?.close(cancellationReason()); session.process = undefined; };
      signal.addEventListener("abort", abort, { once: true });
      try {
        if (!session.process) {
          signal.throwIfAborted();
          if (disposed) throw new Error("CUA attachment is closed");
          session.process = new ComputerProcess(executable, args, environment);
          const discovered = await session.process.start();
          const matches = canonical(discovered) === canonical(catalog);
          if (!matches) throw new Error("CUA provider catalog changed; reconnect the attachment before invoking it");
        }
        signal.throwIfAborted();
        dispatched = true;
        if (name !== "turn_ended" && typeof context.turnId === "string" && context.turnId.trim()) session.turns.set(context.turnId, { callId: context.callId, model: context.model });
        const result = await session.process.rpc("tools/call", {
          name, arguments: input,
          _meta: { "x-codex-turn-metadata": {
            session_id: id,
            ...(context.turnId == null ? {} : { turn_id: context.turnId }),
            thread_id: id, call_id: context.callId, model: context.model,
          } },
        });
        signal.throwIfAborted();
        const content = outputContent(result);
        if (name === "js_reset" && result.isError !== true) session.interrupted = false;
        return toolResult(content, result, { value: result, success: result.isError !== true, metadata: result._meta });
      } catch (error) {
        session.process?.close(); session.process = undefined; session.turns.clear();
        if (dispatched) {
          session.interrupted = true;
          throw new Error(`CUA call interrupted after dispatch. ${interruptedGuidance}`, { cause: error });
        }
        throw error;
      } finally {
        signal.removeEventListener("abort", abort);
      }
    };


    const result = session.tail.then(run);
    session.tail = result.catch(() => {});


    return interruptible(result, signal, cancellationReason);
  };

  const endTurn = (sessionId, turnId, hookEventName = "Stop") => {
    const session = sessions.get(sessionId);
    if (!session || disposed) return Promise.resolve();
    const run = async () => {
      const previous = session.turns.get(turnId);
      session.turns.delete(turnId);
      if (!previous || !session.process || session.interrupted || sessions.get(sessionId) !== session) return;
      const hook = catalog.find(tool => tool.name === "turn_ended");
      if (!hook) return;
      if (!["Stop", "Interrupt", "SubagentStop"].includes(hookEventName)) throw new Error("Unsupported CUA turn lifecycle event");
      const arguments_ = { hook_event_name: hookEventName, session_id: sessionId, turn_id: turnId };
      if (hook.inputSchema.required?.some(key => !Object.hasOwn(arguments_, key))) throw new Error("CUA turn_ended requires unsupported provider arguments");
      try {
        const result = await session.process.rpc("tools/call", {
          name: "turn_ended", arguments: arguments_,
          _meta: {
            "x-codex-turn-metadata": { session_id: sessionId, thread_id: sessionId, turn_id: turnId, call_id: previous.callId, model: previous.model } },
        });
        if (result.isError === true) throw new Error("CUA turn cleanup failed; no retry was attempted");
      } catch (error) {
        session.process?.close(); session.process = undefined; session.turns.clear(); session.interrupted = true;
        throw error;
      }
    };
    const result = session.tail.then(run);
    session.tail = result.catch(() => {});
    return result;
  };
  const allTools = catalog.map(tool => namedTool(`mcp__cua_repl__${tool.name}`, {
      description: tool.description ?? "", parameters: tool.inputSchema,
      ...(tool.outputSchema === undefined ? {} : { outputSchema: tool.outputSchema }),
      providerDefinition: tool,
      supportsParallelToolCalls: true,
      handler: (input, context) => invoke(tool.name, input, context), releaseSession, endTurn, dispose: close,
    }));
  return Object.freeze({
    close, endTurn,
    definitions: catalog,
    tools: allTools.filter((_, index) => modelVisible(catalog[index])),
    tool: name => allTools.find(tool => tool.name === name || tool.name === `mcp__cua_repl__${name}`),
  });
}

function interruptible(result, signal, cancellationReason) {
  return new Promise((resolve, reject) => {
    const abort = () => reject(cancellationReason());
    if (signal.aborted) abort();
    else signal.addEventListener("abort", abort, { once: true });
    result.then(resolve, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

class ComputerProcess {
  constructor(executable, args, environment) {
    const env = Object.fromEntries(["PATH", "HOME", "USER", "LOGNAME", "TMPDIR", "TEMP", "SystemRoot", "LOCALAPPDATA", "LANG", "SKY_ENABLE_AUDIO"]
      .filter(key => process.env[key] !== undefined).map(key => [key, process.env[key]]));
    this.child = spawn(executable, args, { env: { ...env, ...environment }, stdio: ["pipe", "pipe", "ignore"], windowsHide: true });
    this.pending = new Map(); this.sequence = 0; this.lines = new LineBuffer();
    this.child.stdout.on("data", chunk => {
      try {
        this.lines.push(chunk);
        for (let line; (line = this.lines.shift()) !== undefined;) {
          const value = JSON.parse(line.toString("utf8"));
          if (value.method) {
            if (value.id !== undefined) void this.serverRequest(value).catch(error => this.close(error));
            continue;
          }
          const pending = this.pending.get(value.id);
          if (!pending) throw new Error("CUA response belongs to an unknown call");
          this.pending.delete(value.id);
          if (value.error) pending.reject(new Error(value.error.message ?? "CUA runtime error"));
          else pending.resolve(value.result);
        }
      } catch (error) { this.close(error); }
    });
    this.child.once("error", error => this.close(error));
    this.child.once("exit", () => this.close(new Error("CUA runtime exited")));
    this.child.stdin.on("error", error => this.close(error));
  }
  async serverRequest(request) {
    await this.send({ jsonrpc: "2.0", id: request.id, error: { code: -32601, message: "Method not found" } });
  }
  async start() {

    const timer = setTimeout(() => this.close(new Error("CUA provider startup/discovery timed out after 120000 ms")), 120_000);
    timer.unref();
    try {
      await this.initialize();
      return await this.discover();
    } finally { clearTimeout(timer); }
  }
  async initialize() {
    await this.rpc("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "nanocode-cua", version: "0.1.0" } });
    await this.send({ jsonrpc: "2.0", method: "notifications/initialized" });
  }
  async discover() {
    const tools = [], cursors = new Set();
    let cursor;
    do {
      const page = await this.rpc("tools/list", cursor ? { cursor } : {});
      if (!Array.isArray(page.tools)) throw new Error("CUA provider did not return an MCP tools/list catalog");
      tools.push(...page.tools);
      cursor = page.nextCursor;
      if (cursor !== undefined && (typeof cursor !== "string" || !cursor || cursors.has(cursor))) throw new Error("CUA provider returned an invalid or repeated tools/list cursor");
      cursors.add(cursor);
    } while (cursor !== undefined);
    return validateCatalog(tools);
  }

  send(value) {
    if (this.closed) throw this.closed;
    const data = JSON.stringify(value) + "\n";
    return new Promise((resolve, reject) => {
      this.child.stdin.write(data, error => error ? reject(error) : resolve());
    });
  }
  async rpc(method, params) {
    const id = ++this.sequence;
    const response = new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
    try { await this.send({ jsonrpc: "2.0", id, method, params }); }
    catch (error) { this.pending.delete(id); throw error; }
    return response;
  }
  close(error = new Error("CUA session stopped")) {
    if (this.closed) return;
    this.closed = error;
    this.child.kill("SIGKILL");
    this.child.stdin.destroy(); this.child.stdout.destroy();
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear(); this.lines.clear();
  }
}




class LineBuffer {
  chunks = [];
  push(chunk) { if (chunk.length) this.chunks.push(chunk); }
  shift() {
    let length = 0;
    for (let index = 0; index < this.chunks.length; index++) {
      const chunk = this.chunks[index];
      const newline = chunk.indexOf(10);
      if (newline === -1) { length += chunk.length; continue; }
      const line = Buffer.allocUnsafe(length + newline);
      let offset = 0;
      for (let part = 0; part < index; part++) {
        this.chunks[part].copy(line, offset);
        offset += this.chunks[part].length;
      }
      chunk.copy(line, offset, 0, newline);
      const remainder = chunk.subarray(newline + 1);
      this.chunks = [
        ...(remainder.length ? [remainder] : []),
        ...this.chunks.slice(index + 1),
      ];
      return line;
    }
  }
  clear() { this.chunks = []; }
}

function outputContent(result) {
  if (!Array.isArray(result?.content)) throw new TypeError("CUA result requires content");
  return result.content.map(item => {
    if (item.type === "text" && typeof item.text === "string") return { type: "input_text", text: item.text };
    if (item.type === "image" && typeof item.mimeType === "string" && typeof item.data === "string") {
      return { type: "input_image", image_url: `data:${item.mimeType};base64,${item.data}`, detail: "original" };
    }
    if (item.type === "audio" && typeof item.mimeType === "string" && typeof item.data === "string") {
      return { type: "input_audio", audio_url: `data:${item.mimeType};base64,${item.data}` };
    }


    return { type: "input_text", text: JSON.stringify(item) };
  });
}

function canonical(value) {
  return JSON.stringify(value, function (_key, item) {
    return item && typeof item === "object" && !Array.isArray(item)
      ? Object.fromEntries(Object.keys(item).sort().map(key => [key, item[key]])) : item;
  });
}

function validateCatalog(tools) {
  const names = new Set();
  for (const tool of tools) {
    if (typeof tool?.name !== "string" || !tool.name || names.has(tool.name)) throw new Error("CUA provider tool names must be non-empty and unique");
    if (tool.description !== undefined && typeof tool.description !== "string") throw new Error("CUA provider tool description must be a string");
    if (!tool.inputSchema || typeof tool.inputSchema !== "object" || Array.isArray(tool.inputSchema)) throw new Error("CUA provider tool inputSchema must be a JSON schema object");
    names.add(tool.name);
  }
  return structuredClone(tools);
}

function modelVisible(tool) {
  const visibility = tool._meta?.ui?.visibility;
  return !Array.isArray(visibility) || visibility.includes("model");
}

export const connect = connectComputerTools;
export const CUA_JS_NAME = "mcp__cua_repl__js";
export const CUA_RESET_NAME = "mcp__cua_repl__js_reset";
