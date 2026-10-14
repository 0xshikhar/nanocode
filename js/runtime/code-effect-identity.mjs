

export function createCodeEffectIdentity(enabled) {
  const sessions = new Map();
  const acceptedTurns = new Map();
  const waiters = new Set();
  function sessionFor(sessionId) {
    let session = sessions.get(sessionId);
    if (!session) { session = { turns: new Map(), aliases: new Map() }; sessions.set(sessionId, session); }
    return session;
  }
  function unavailable(retryable = false) {
    throw Object.assign(new Error("Original operation/model-call identity unavailable; effect journal interrupted"), { code: "host_interrupted", retryable });
  }
  function lookup(sessionId, parentCallId, turnId) {
    const session = sessions.get(sessionId);
    let projected = session?.turns.has(turnId) ? turnId : session?.aliases.get(turnId);
    if (!projected) {



      const prefix = sessionId + ":";
      if (typeof turnId !== "string" || !turnId.startsWith(prefix) || !/^[1-9]\d*$/.test(turnId.slice(prefix.length))) unavailable();
      const candidates = [...session?.turns ?? []].filter(([, turn]) => turn.calls.get(parentCallId)?.pending);
      if (candidates.length !== 1) unavailable(candidates.length === 0);
      projected = candidates[0][0];
      session.aliases.set(turnId, projected);
    }
    if (acceptedTurns.get(projected) !== sessionId) unavailable();
    const identity = session.turns.get(projected)?.calls.get(parentCallId);
    if (!identity || !identity.pending) unavailable(true);
    if (typeof identity.operationId !== "string" || !identity.operationId) unavailable();
    identity.pending = false;
    return { operationId: identity.operationId, modelCallIndex: identity.modelCallIndex };
  }
  function notify() { for (const waiter of [...waiters]) waiter.check(); }
  return {
    observe(encoded) {
      if (!enabled) return;
      const event = typeof encoded === "string" ? JSON.parse(encoded) : encoded;
      const payload = event?.payload;
      if (typeof payload?.turn_id !== "string") return;
      if (event.type === "input.accepted" && payload.kind === "prompt") {
        const sessionId = payload.session_id;
        if (typeof sessionId !== "string") return;
        const session = sessionFor(sessionId);
        const previous = acceptedTurns.get(payload.turn_id);
        acceptedTurns.set(payload.turn_id, previous === undefined || previous === sessionId ? sessionId : null);
        if (!session.turns.has(payload.turn_id)) {
          if (session.turns.size >= 128) {
            const old = session.turns.keys().next().value;
            session.turns.delete(old);
            if (acceptedTurns.get(old) === sessionId) acceptedTurns.delete(old);
            for (const [alias, projected] of session.aliases) if (projected === old) session.aliases.delete(alias);
          }




          const operationId = typeof payload.request_id === "string" && payload.request_id
            ? payload.request_id
            : payload.request_id === null && payload.item_id === payload.turn_id + ":prompt"
              ? "non-durable:" + payload.item_id : undefined;
          session.turns.set(payload.turn_id, { operationId, calls: new Map() });
        }
      } else if (event.type === "tool.call") {



        const sessionId = acceptedTurns.get(payload.turn_id);
        if (typeof sessionId !== "string") return;
        const turn = sessions.get(sessionId)?.turns.get(payload.turn_id);
        if (payload.session_id !== undefined && payload.session_id !== sessionId) {
          turn?.calls.delete(payload.call_id);
          notify(); return;
        }
        if (!turn || typeof payload.call_id !== "string") return;
        if (!Number.isSafeInteger(payload.model_call_index) || payload.model_call_index < 1) {
          turn.calls.delete(payload.call_id);
          notify(); return;
        }
        if (turn.calls.size >= 128 && !turn.calls.has(payload.call_id)) turn.calls.delete(turn.calls.keys().next().value);
        turn.calls.set(payload.call_id, { operationId: turn.operationId, modelCallIndex: payload.model_call_index, pending: true });
      }
      notify();
    },
    async resolve(sessionId, parentCallId, turnId, signal) {
      signal?.throwIfAborted();
      try { return lookup(sessionId, parentCallId, turnId); }
      catch (error) { if (!error.retryable || waiters.size >= 128) throw error; }



      return new Promise((resolve, reject) => {
        let timer;
        const waiter = { sessionId, check() {
          try { settle(null, lookup(sessionId, parentCallId, turnId)); }
          catch (error) { if (!error.retryable) settle(error); }
        }, cancel: () => settle(Object.assign(new Error("Effect identity wait interrupted"), { code: "host_interrupted" })) };
        function settle(error, identity) {
          if (!waiters.delete(waiter)) return;
          clearTimeout(timer); signal?.removeEventListener("abort", abort);
          if (error) reject(error); else resolve(identity);
        }
        const abort = () => settle(signal.reason);
        waiters.add(waiter);
        signal?.addEventListener("abort", abort, { once: true });
        timer = setTimeout(() => {
          try { unavailable(); } catch (error) { settle(error); }
        }, 1000);
        if (signal?.aborted) abort(); else waiter.check();
      });
    },
    release(sessionId) {
      for (const waiter of [...waiters]) if (waiter.sessionId === sessionId) waiter.cancel();
      for (const turn of sessions.get(sessionId)?.turns.keys() ?? []) {
        if (acceptedTurns.get(turn) === sessionId) acceptedTurns.delete(turn);
      }
      sessions.delete(sessionId);
    },
    reset() { for (const waiter of [...waiters]) waiter.cancel(); sessions.clear(); acceptedTurns.clear(); },
  };
}
