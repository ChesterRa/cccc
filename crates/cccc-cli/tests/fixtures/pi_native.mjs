import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";
import { runInNewContext } from "node:vm";

const handlers = new Map(), children = [], errors = [], submissions = [];
let session = "fixture-session", outcome = "queued";
const spawn = () => {
  const child = new EventEmitter();
  child.stdin = new PassThrough();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.receipts = [];
  createInterface({ input: child.stdin }).on("line", line => child.receipts.push(JSON.parse(line)));
  child.stdin.on("finish", () => { child.stdout.end(); child.emit("close", 0); });
  child.kill = () => child.stdin.end();
  children.push(child);
  setImmediate(() => child.stdout.write('{"type":"ready"}\n'));
  return child;
};
const sandbox = {
  spawn, createInterface, getAgentDir: () => "/fixture/pi", process: { pid: 123, env: {} },
  setTimeout, clearTimeout, __CCCC_EXECUTABLE__: "/fixture/cccc", __CCCC_HOME__: "/fixture/cccc-home",
};
runInNewContext(readFileSync(process.argv[2], "utf8")
  .replace(/^import .*;\n/gm, "").replaceAll("new Promise<void>", "new Promise")
  .replace("export default", "globalThis.plugin ="), sandbox);
const emit = async (type, data) => handlers.get(type)?.(data, ctx);
const ctx = { cwd: "/fixture/repo", sessionManager: { getSessionId: () => session }, ui: { notify() {} } };
sandbox.plugin({
  on: (type, handler) => handlers.set(type, handler),
  sendUserMessage: (text, options) => {
    submissions.push({ text, options });
    if (outcome === "throw") throw new Error("synchronous rejection");
    if (outcome !== "queued") {
      // Pi's public ExtensionAPI returns void, even when its underlying
      // async prompt rejects. Its wrapper owns this catch, not the caller.
      Promise.reject(new Error(outcome)).catch(error => errors.push(error.message));
    }
  },
});
const settle = async () => { for (let i = 0; i < 4; i++) await new Promise(setImmediate); };
const handoff = id => children.at(-1).stdout.write(JSON.stringify({ handoff: { handoff_id: id, text: `CCCC event ${id}` } }) + "\n");
const user = text => emit("message_start", { message: { role: "user", content: [{ type: "text", text }] } });
await emit("session_start", {});
const child = children[0];
for (const error of ["compaction in progress", "authentication failed"]) {
  outcome = error;
  handoff(error);
  await settle();
  assert.equal(child.receipts.length, 0, "void wrapper cannot acknowledge async rejection");
}
assert.equal(errors.length, 2);
outcome = "queued";
handoff("first");
await settle();
assert.equal(child.receipts.length, 0, "queue call alone is not admission evidence");
await emit("message_start", { message: { role: "assistant", content: [{ type: "text", text: "CCCC event first" }] } });
await user("another input");
assert.equal(child.receipts.length, 0, "unrelated messages are not evidence");
await user("CCCC event first");
await settle();
assert.deepEqual(child.receipts, [{ handoff_id: "first", state: "accepted" }]);
await user("CCCC event first");
assert.equal(child.receipts.length, 1, "admission is acknowledged once");
handoff("next");
await settle();
await user("CCCC event first");
assert.equal(child.receipts.length, 1, "late old events cannot acknowledge the next handoff");
await user("CCCC event next");
await settle();
assert.deepEqual(child.receipts.at(-1), { handoff_id: "next", state: "accepted" });
handoff("group-a");
handoff("group-b");
await settle();
await user("CCCC event group-b");
await user("CCCC event group-a");
await settle();
assert.deepEqual(child.receipts.slice(-2), [
  { handoff_id: "group-b", state: "accepted" },
  { handoff_id: "group-a", state: "accepted" },
]);
outcome = "throw";
handoff("rejected");
await settle();
assert.deepEqual(child.receipts.at(-1), { handoff_id: "rejected", state: "rejected" });
outcome = "queued";
handoff("stale");
await settle();
session = "replacement-session";
await user("CCCC event stale");
assert.equal(child.receipts.length, 5, "session replacement fences an old receipt");
await emit("session_shutdown", {});
await user("CCCC event stale");
assert.equal(child.receipts.length, 5);
assert.equal(child.stdin.writableEnded, true);
assert.ok(submissions.every(call => call.options.deliverAs === "followUp"));
