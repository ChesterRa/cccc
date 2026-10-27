import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";
import { runInNewContext } from "node:vm";

// Native Promise API fixtures: session/info/prompt return values directly;
// location-scoped plugin and MCP listings return {location, data}.
const children = [];
let effect;
let id = "ses_original";
let promptError;
let replyID;
const prompts = [];
const spawn = (_executable, args) => {
  const child = new EventEmitter();
  child.stdin = new PassThrough();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.receipts = [];
  child.stdin.on("data", bytes => child.receipts.push(JSON.parse(bytes)));
  child.stdin.on("finish", () => {
    child.stdout.end();
    child.emit("close", 0);
  });
  child.kill = () => child.stdin.end();
  children.push({ child, args });
  return child;
};
const sandbox = {
  spawn, createInterface, createEffect: callback => { effect = callback; callback(); },
  process: { pid: 123, env: {} }, setTimeout, clearTimeout,
  __CCCC_EXECUTABLE__: "/fixture/cccc", __CCCC_HOME__: "/fixture/home",
  __OPENCODE_CONFIG__: "/fixture/config/opencode",
};
const source = readFileSync(process.argv[2], "utf8")
  .replace(/^import .*;\n/gm, "").replace("export default", "globalThis.plugin =");
runInNewContext(source, sandbox);
const ctx = {
  client: {
    session: {
      get: async ({ sessionID }) => ({ id: sessionID, location: { directory: "/fixture/repo" } }),
      prompt: async input => {
        prompts.push(input);
        if (promptError) throw promptError;
        return { sessionID: replyID ?? input.sessionID };
      },
    },
    server: { info: async () => ({ pid: 456 }) },
    plugin: { list: async () => ({ data: [{ id: "cccc.native", state: { status: "active" } }] }) },
    mcp: { list: async () => ({ data: [{ name: "cccc", status: { status: "connected" } }] }) },
  },
  data: {
    listen: () => () => {},
    session: { get: () => ({ location: { directory: "/fixture/repo" } }) },
  },
  ui: {
    toast: { show() {} },
    router: { current: () => ({ type: "session", sessionID: id }) },
    tabs: { enabled: () => true, list: () => [{ sessionID: id, title: "Original TUI" }] },
    slot: ({ render }) => { render(); return () => {}; },
  },
};
const settle = async () => { for (let i = 0; i < 8; i++) await new Promise(setImmediate); };
const cleanup = sandbox.plugin.setup(ctx);
await settle();
assert.equal(children.length, 1, "route and tab identify one session");
assert.equal(children[0].args[children[0].args.indexOf("--session-id") + 1], id);
assert.equal(children[0].args[children[0].args.indexOf("--backend-pid") + 1], "456");
for (const [error, returnedID, state] of [
  [undefined, undefined, "accepted"],
  [{ _tag: "SessionNotFoundError" }, undefined, "rejected"],
  [{ _tag: "InternalError" }, undefined, "ambiguous"],
  [undefined, "ses_wrong", "ambiguous"],
]) {
  promptError = error;
  replyID = returnedID;
  const child = children[0].child;
  child.stdout.write(JSON.stringify({ handoff: { handoff_id: state, text: "Synthetic input" } }) + "\n");
  await settle();
  assert.equal(child.receipts.at(-1).state, state);
  assert.equal(prompts.at(-1).sessionID, "ses_original");
  assert.equal(prompts.at(-1).delivery, "queue");
}
id = "ses_second";
effect();
await settle();
assert.equal(children[0].child.stdin.writableEnded, true, "closed source releases its bridge");
assert.equal(children.length, 2);
await cleanup();
assert.equal(children[1].child.stdin.writableEnded, true, "TUI cleanup releases its bridge");
ctx.client.mcp.list = async () => ({ data: [{ name: "cccc", status: { status: "disabled" } }] });
const stop = sandbox.plugin.setup(ctx);
await settle();
assert.equal(children.length, 2, "disabled MCP never advertises receiving");
await stop();
sandbox.process.env.CCCC_ACTOR_ID = "managed";
assert.equal(sandbox.plugin.setup(ctx), undefined, "managed Actors retain their existing integration");

// An existing Actor-scoped MCP is not an unbound registration. Keeping its
// identity environment would make cccc ignore --local-session-runtime.
const backend = {
  process: { env: {} },
  __CCCC_EXECUTABLE__: "/fixture/cccc", __CCCC_HOME__: "/fixture/home",
};
const backendSource = readFileSync(process.argv[2].replace("-tui.ts", ".ts"), "utf8")
  .replace("export default", "globalThis.plugin =");
runInNewContext(backendSource, backend);
for (const scoped of [
  {},
  { CCCC_ACTOR_ID: "fixture-managed-actor" },
  { CCCC_SECRETARY_TASK_TOKEN_FILE: "/fixture/task-authority" },
  { CCCC_MCP_TOOL_PROFILE: "secretary-task" },
]) {
  const previous = {
    type: "local", command: ["/fixture/cccc", "mcp"],
    environment: { CCCC_HOME: "/fixture/home", NATIVE_FIXTURE_SETTING: "keep", ...scoped },
  };
  const before = JSON.stringify(previous);
  let written;
  const setup = () => backend.plugin.setup({
    mcp: { transform: async callback => callback({
      get: () => previous, set: (_name, value) => { written = value; },
    }) },
  });
  if (Object.keys(scoped).length) {
    await assert.rejects(setup, /another configuration/);
    assert.equal(written, undefined, "scoped registration stays unchanged");
  } else {
    await setup();
    assert.equal(written.command[2], "--local-session-runtime");
    assert.equal(written.environment.NATIVE_FIXTURE_SETTING, "keep");
  }
  assert.equal(JSON.stringify(previous), before, "native settings remain owned by the user");
}
