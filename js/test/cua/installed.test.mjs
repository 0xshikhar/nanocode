import test from "node:test";
import assert from "node:assert/strict";
import { connect } from "../../node/cua.mjs";

test("installed default provider executes Sky imports in a persistent REPL", {
  skip: process.env.NANOCODE_TEST_INSTALLED_CUA !== "1",
  timeout: 30_000,
}, async t => {
  const cua = await connect();
  t.after(() => cua.close());
  assert.ok(cua.tools.some(tool => tool.name === "mcp__cua_repl__js"));
  const context = { sessionId: "default-cua-journey", turnId: "verify", callId: "verify", model: "fixture", signal: new AbortController().signal };
  const first = await cua.tool("js").handler({ code: 'var installedSky = await import("@oai/sky"); console.log(Object.keys(installedSky).length > 0)', title: "Verify installed Sky module" }, context);
  assert.equal(first.success, true, JSON.stringify(first));
  assert.ok(first.output.some(item => item.text?.includes("true")), JSON.stringify(first));
  const second = await cua.tool("js").handler({ code: 'console.log(typeof installedSky)', title: "Verify retained REPL state" }, context);
  assert.equal(second.success, true, JSON.stringify(second));
  assert.ok(second.output.some(item => item.text?.includes("object")), JSON.stringify(second));
});
