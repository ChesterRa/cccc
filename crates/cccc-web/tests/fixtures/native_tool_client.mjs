// Disposable host-side client. Credentials travel over stdin/HTTP only; they
// never enter argv, files, user output, or CCCC's business source.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";

const config = JSON.parse(readFileSync(0, "utf8"));
assert.equal(new URL(config.url).hostname, "127.0.0.1");
let stage = "claim";
let claimCount = 0;

async function invoke(name, args) {
  if (name === "cccc_runtime_wait_next_turn") claimCount += 1;
  const response = await fetch(config.url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${config.secret}`,
    },
    body: JSON.stringify({
      jsonrpc: "2.0", id: "host-fixture", method: "tools/call",
      params: { name, arguments: args, _meta: config.metadata },
    }),
  });
  assert.equal(response.ok, true);
  const envelope = await response.json();
  assert.equal(Boolean(envelope.error), false);
  const result = envelope.result;
  const payload = result.structuredContent ?? JSON.parse(result.content[0].text);
  return { ok: result.isError !== true, payload };
}

async function main() {
  let first = await invoke("cccc_runtime_wait_next_turn", {});
  assert.equal(first.ok, true);
  assert.equal(first.payload.status, "work_available");
  assert.equal(typeof first.payload.actor_token, "string");
  assert.equal(first.payload.actor_token.length, 43);

  // The host retains its original tool output independently of the script's
  // variables. One mode deliberately loses the structured-object reference.
  let originalToolOutput = JSON.stringify(first.payload);
  let context = config.mode === "retained-reference" ? first.payload : null;
  first = null;

  async function call(name, argumentsWithoutCredential) {
    const current = context ?? JSON.parse(originalToolOutput);
    const args = { ...argumentsWithoutCredential, actor_token: current.actor_token };
    if (config.mode === "retained-reference") return invoke(name, args);
    // A fresh host code cell may put the current value directly in a tool's
    // argument. This source runs in the host, never inside cccc_code_exec.
    return runInNewContext(
      `invoke(${JSON.stringify(name)}, ${JSON.stringify(args)})`, { invoke },
    );
  }

  stage = "business-code";
  const businessSource = "const c = await tools.cccc_context_get({}); text({hasContext: Boolean(c)});";
  assert.equal(businessSource.includes(JSON.parse(originalToolOutput).actor_token), false);
  const business = await call("cccc_code_exec", { source: businessSource });
  assert.equal(business.ok, true);
  assert.equal(business.payload.status, "completed");

  stage = "reply";
  const turn = JSON.parse(originalToolOutput).turn;
  const reply = await call("cccc_message_reply", {
    event_id: turn.event_ids[0], text: "native host credential handoff fixture",
  });
  assert.equal(reply.ok, true);

  stage = "complete";
  const complete = await call("cccc_runtime_complete_turn", {
    turn_id: turn.turn_id, event_ids: turn.event_ids, status: "done",
  });
  assert.equal(complete.ok, true);
  assert.equal(complete.payload.status, "done");
  assert.equal(claimCount, 1);
  context = null;
  originalToolOutput = null;
  process.stdout.write(JSON.stringify({
    mode: config.mode, claim_count: claimCount, code_completed: true,
    reply_completed: true, turn_completed: true, temporary_context_discarded: true,
  }));
}

main().catch((error) => {
  // Failed tool results can include a credential. Print only a fixed stage and
  // error class, never the payload, generated host source, or assertion values.
  process.stderr.write(`native host fixture failed at ${stage}: ${error.name}\n`);
  process.exitCode = 1;
});
