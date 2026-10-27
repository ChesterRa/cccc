import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { createEffect } from "solid-js";

const executable = __CCCC_EXECUTABLE__;
const home = __CCCC_HOME__;
const providerHome = __OPENCODE_CONFIG__;

export default {
  id: "cccc.native.tui",
  setup(ctx) {
    if (["CCCC_ACTOR_ID", "CCCC_SECRETARY_TASK_TOKEN", "CCCC_SECRETARY_TASK_TOKEN_FILE"].some(k => process.env[k]) || process.env.CCCC_MCP_TOOL_PROFILE === "secretary-task") return;
    const bridges = new Map();
    let desired = new Set();
    let disposed = false;
    let revision = 0;
    let selectionKey = "";
    let pending = Promise.resolve();
    const notify = message => ctx.ui.toast.show({ message, variant: "warning" });

    async function start(sessionID) {
      const session = await ctx.client.session.get({ sessionID });
      const location = session.location;
      if (!location?.directory || disposed || !desired.has(sessionID)) return;
      const [info, plugins, servers] = await Promise.all([
        ctx.client.server.info(),
        ctx.client.plugin.list({ location }),
        ctx.client.mcp.list({ location }),
      ]);
      const ready = plugins.data?.some(plugin => plugin.id === "cccc.native" && plugin.state?.status === "active");
      const connected = servers.data?.some(server => server.name === "cccc" && server.status?.status === "connected");
      if (!ready || !connected || disposed || !desired.has(sessionID)) return;
      const backendPID = info.pid;
      if (!Number.isInteger(backendPID) || backendPID <= 0) return;
      const child = spawn(executable, ["sessions", "extension-bridge", "--runtime", "opencode",
        "--host-pid", String(process.pid), "--backend-pid", String(backendPID),
        "--session-id", sessionID, "--cwd", location.directory, "--provider-home", providerHome], {
        env: { ...process.env, CCCC_HOME: home }, stdio: ["pipe", "pipe", "pipe"],
      });
      let active = true;
      const lines = createInterface({ input: child.stdout });
      child.stdin.on("error", () => {});
      child.stderr.resume();
      const exited = new Promise(resolve => child.once("close", () => { active = false; resolve(); }));
      child.once("error", () => notify("CCCC native access is unavailable. Check `cccc sessions list --include-offline`."));
      const bridge = {
        directory: location.directory,
        async close() {
          active = false;
          if (!child.stdin.destroyed) child.stdin.end();
          const timer = setTimeout(() => child.kill(), 5000);
          try { await exited; } finally { clearTimeout(timer); lines.close(); }
        },
      };
      bridges.set(sessionID, bridge);
      child.once("close", () => {
        if (bridges.get(sessionID) === bridge) bridges.delete(sessionID);
      });
      lines.on("line", async line => {
        let value;
        try { value = JSON.parse(line); } catch { child.kill(); return; }
        if (value.type === "ready") return;
        const handoff = value.handoff;
        if (!handoff || !active || disposed || !desired.has(sessionID)) return;
        let state = "ambiguous";
        try {
          // Keep the checked native ID fixed across awaits and use its own
          // location, rather than the terminal's currently focused directory.
          const response = await ctx.client.session.prompt({
            sessionID, location, text: handoff.text, delivery: "queue",
          });
          if (response.sessionID === sessionID) {
            state = "accepted";
          }
        } catch (error) {
          // Only explicit validation/identity rejection proves non-admission.
          // An internal or transport error can follow a durable write.
          if (["InvalidRequestError", "SessionNotFoundError", "LocationNotFoundError", "UnauthorizedError"].includes(error?._tag)) state = "rejected";
        }
        if (!child.stdin.destroyed) child.stdin.write(JSON.stringify({ handoff_id: handoff.handoff_id, state }) + "\n");
        if (state !== "accepted") notify("CCCC could not confirm native input admission. Check this Actor's Inbox before retrying.");
      });
    }

    function sync() {
      const current = ++revision;
      const selected = new Set(desired);
      pending = pending.then(async () => {
        if (disposed || current !== revision) return;
        for (const [id, bridge] of bridges) {
          const currentDirectory = ctx.data.session.get(id)?.location?.directory;
          if (selected.has(id) && (!currentDirectory || currentDirectory === bridge.directory)) continue;
          bridges.delete(id);
          await bridge.close();
        }
        for (const id of selected) {
          if (disposed || current !== revision) return;
          if (!bridges.has(id)) await start(id);
        }
      }).catch(() => notify("CCCC native access is unavailable. Reload the OpenCode integration and check its MCP connection."));
    }

    const stopEvents = ctx.data.listen(({ details }) => {
      if (details.type.startsWith("mcp.") || details.type === "plugin.updated" || details.type === "location.ready" || details.type === "server.connected") sync();
      if (details.type === "session.deleted") {
        desired.delete(details.data.sessionID);
        sync();
      }
    });
    const stopSlot = ctx.ui.slot({ append: "app", render: () => {
      createEffect(() => {
        const route = ctx.ui.router.current();
        const ids = new Set();
        if (ctx.ui.tabs.enabled()) for (const tab of ctx.ui.tabs.list()) ids.add(tab.sessionID);
        if (route.type === "session") ids.add(route.sessionID);
        const key = JSON.stringify([...ids].sort().map(id => [id, ctx.data.session.get(id)?.location]));
        if (key === selectionKey) return;
        selectionKey = key;
        desired = ids;
        sync();
      });
      return null;
    }});
    return async () => {
      disposed = true;
      revision++;
      stopEvents();
      stopSlot();
      const closing = [...bridges.values()].map(bridge => bridge.close());
      bridges.clear();
      await Promise.all(closing);
    };
  },
};
