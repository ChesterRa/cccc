import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { getAgentDir } from "@earendil-works/pi-coding-agent";

// Installed by `cccc sessions enable --runtime pi`; native configuration stays in Pi.
const executable = __CCCC_EXECUTABLE__;
const home = __CCCC_HOME__;

export default function (pi) {
  let close;
  let observeInput;
  pi.on("message_start", event => observeInput?.(event.message));
  pi.on("session_start", async (_event, ctx) => {
    if (["CCCC_ACTOR_ID", "CCCC_SECRETARY_TASK_TOKEN", "CCCC_SECRETARY_TASK_TOKEN_FILE"].some(k => process.env[k]) || process.env.CCCC_MCP_TOOL_PROFILE === "secretary-task") return;
    const session = ctx.sessionManager.getSessionId();
    const child = spawn(executable, ["sessions", "extension-bridge", "--runtime", "pi", "--host-pid", String(process.pid), "--session-id", session, "--cwd", ctx.cwd, "--provider-home", getAgentDir()], {
      env: { ...process.env, CCCC_HOME: home }, stdio: ["pipe", "pipe", "pipe"],
    });
    let active = true;
    let ready;
    const started = new Promise<void>((resolve, reject) => {
      ready = resolve;
      child.once("error", reject);
      child.once("exit", code => reject(new Error(`CCCC receive bridge exited (${code})`)));
    });
    const lines = createInterface({ input: child.stdout });
    // A bridge can exit between admission and its ACK. Losing that receipt is
    // uncertain delivery, not an unhandled EPIPE in the user's Pi process.
    child.stdin.on("error", () => {});
    // Do not mirror CLI diagnostics or message bodies into another conversation.
    child.stderr.resume();
    const pending = new Map();
    const acknowledge = (handoff, state) => {
      pending.delete(handoff.handoff_id);
      if (!child.stdin.destroyed) child.stdin.write(JSON.stringify({ handoff_id: handoff.handoff_id, state }) + "\n");
    };
    const observe = message => {
      if (!active || ctx.sessionManager.getSessionId() !== session || message.role !== "user") return;
      const text = message.content.filter(part => part.type === "text").map(part => part.text).join("\n");
      for (const handoff of pending.values()) {
        if (text === handoff.text) { acknowledge(handoff, "accepted"); break; }
      }
    };
    observeInput = observe;
    lines.on("line", line => {
      let value;
      try { value = JSON.parse(line); } catch { child.kill(); return; }
      if (value.type === "ready") { ready(); return; }
      const handoff = value.handoff;
      if (!handoff || !active || ctx.sessionManager.getSessionId() !== session) return;
      // The public extension API returns void and catches asynchronous prompt
      // failures internally. Only Pi's exact native user-message event proves
      // admission. A queued input not observed before the bridge's handoff
      // deadline stays ambiguous; never replay it or infer a receipt from output.
      // One native source may receive from several Groups. Retain their exact
      // receipt correlations, bounded like the daemon's receive queue. Evicted
      // unconfirmed inputs remain ambiguous, never accepted or replayed.
      if (pending.size >= 256) pending.delete(pending.keys().next().value);
      pending.set(handoff.handoff_id, handoff);
      try {
        pi.sendUserMessage(handoff.text, { deliverAs: "followUp" });
      } catch {
        acknowledge(handoff, "rejected");
        ctx.ui.notify("CCCC input was rejected by Pi. Check the Actor Inbox.", "warning");
      }
    });
    // close also fires when spawn fails; exit alone does not.
    const exited = new Promise<void>(resolve => child.once("close", () => { active = false; resolve(); }));
    close = async () => {
      active = false;
      pending.clear();
      if (observeInput === observe) observeInput = undefined;
      if (!child.stdin.destroyed) child.stdin.end();
      const timer = setTimeout(() => child.kill(), 5000);
      try { await exited; } finally { clearTimeout(timer); lines.close(); }
    };
    try { await started; } catch {
      ctx.ui.notify("CCCC native receive is unavailable. Check `cccc sessions list --include-offline`.", "warning");
    }
  });
  pi.on("session_shutdown", async () => { const stop = close; close = undefined; await stop?.(); });
}
