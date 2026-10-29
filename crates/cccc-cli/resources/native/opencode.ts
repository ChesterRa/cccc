// Installed by `cccc sessions enable --runtime opencode`.
// The server supplies current per-call session metadata; the original TUI owns receiving.
const executable = __CCCC_EXECUTABLE__;
const home = __CCCC_HOME__;

export default {
  id: "cccc.native",
  async setup(ctx) {
    if (["CCCC_ACTOR_ID", "CCCC_SECRETARY_TASK_TOKEN", "CCCC_SECRETARY_TASK_TOKEN_FILE"].some(k => process.env[k]) || process.env.CCCC_MCP_TOOL_PROFILE === "secretary-task") return;
    await ctx.mcp.transform(editor => {
      const previous = editor.get("cccc");
      if (previous) {
        const command = previous.command;
        const args = Array.isArray(command) ? command.slice(1) : [];
        const native = args.length === 3 && args[0] === "mcp" && args[1] === "--local-session-runtime" && args[2] === "opencode";
        const ordinary = args.length === 1 && args[0] === "mcp";
        const scoped = ["CCCC_ACTOR_ID", "CCCC_SECRETARY_TASK_TOKEN", "CCCC_SECRETARY_TASK_TOKEN_FILE"].some(k => previous.environment?.[k]) || previous.environment?.CCCC_MCP_TOOL_PROFILE === "secretary-task";
        if (previous.type !== "local" || command?.[0] !== executable ||
            !(native || ordinary) || previous.environment?.CCCC_HOME !== home || previous.disabled || scoped) {
          throw new Error("CCCC native access could not load: the existing cccc MCP entry belongs to another configuration or is disabled. Resolve it in OpenCode before reloading.");
        }
      }
      editor.set("cccc", {
        ...previous,
        type: "local",
        command: [executable, "mcp", "--local-session-runtime", "opencode"],
        environment: { ...previous?.environment, CCCC_HOME: home },
        codemode: false,
      });
    });
  },
};
