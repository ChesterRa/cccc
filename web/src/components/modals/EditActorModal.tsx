import { RuntimeInfo, SupportedRuntime, SUPPORTED_RUNTIMES, RUNTIME_INFO } from "../../types";
import { BASIC_MCP_CONFIG_SNIPPET, COPILOT_MCP_CONFIG_SNIPPET, OPENCODE_MCP_CONFIG_SNIPPET } from "../../utils/mcpConfigSnippets";

export interface EditActorModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  actorId: string;
  runtimes: RuntimeInfo[];
  runtime: SupportedRuntime;
  onChangeRuntime: (runtime: SupportedRuntime) => void;
  command: string;
  onChangeCommand: (command: string) => void;
  onSave: () => void;
  onCancel: () => void;
}

export function EditActorModal({
  isOpen,
  isDark,
  busy,
  actorId,
  runtimes,
  runtime,
  onChangeRuntime,
  command,
  onChangeCommand,
  onSave,
  onCancel,
}: EditActorModalProps) {
  if (!isOpen) return null;

  const rtInfo = runtimes.find((r) => r.name === runtime);
  const available = rtInfo?.available ?? false;
  const defaultCommand = rtInfo?.recommended_command || "";
  const requireCommand = runtime === "custom" || !available;

  return (
    <div
      className={`fixed inset-0 flex items-start justify-center p-4 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/60" : "bg-black/40"}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="edit-actor-title"
    >
      <div
        className={`w-full max-w-md mt-8 sm:mt-16 rounded-2xl border shadow-2xl animate-scale-in ${
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        }`}
      >
        <div className={`px-6 py-4 border-b ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div id="edit-actor-title" className={`text-lg font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
            Edit Agent: {actorId}
          </div>
          <div className={`text-sm mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Change the AI runtime for this agent</div>
        </div>
        <div className="p-6 space-y-5">
          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Runtime</label>
            <select
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={runtime}
              onChange={(e) => {
                const next = e.target.value as SupportedRuntime;
                onChangeRuntime(next);
                onChangeCommand("");
              }}
            >
              {SUPPORTED_RUNTIMES.map((rt) => {
                const info = RUNTIME_INFO[rt];
                const rtInfo = runtimes.find((r) => r.name === rt);
                const available = rtInfo?.available ?? false;
                const selectable = available || rt === "custom";
                return (
                  <option key={rt} value={rt} disabled={!selectable}>
                    {info?.label || rt}
                    {!available && rt !== "custom" ? " (not installed)" : ""}
                  </option>
                );
              })}
            </select>

            {(runtime === "cursor" || runtime === "kilocode" || runtime === "opencode" || runtime === "copilot" || runtime === "custom") && (
              <div
                className={`mt-2 rounded-xl border px-3 py-2 text-[11px] ${
                  isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
                }`}
              >
                <div className="font-medium">Manual MCP install required</div>
                {runtime === "custom" ? (
                  <>
                    <div className="mt-1">
                      Configure your runtime to add an MCP stdio server named{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc</code>{" "}
                      that runs <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc mcp</code>.
                    </div>
                  </>
                ) : runtime === "cursor" ? (
                  <>
                    <div className="mt-1">
                      1) Create/edit{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.cursor/mcp.json</code> (or{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.cursor/mcp.json</code> in this project)
                    </div>
                    <div className="mt-1">2) Add this MCP server config:</div>
                  </>
                ) : runtime === "kilocode" ? (
                  <>
                    <div className="mt-1">
                      1) Create/edit{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.kilocode/mcp.json</code> in this project root
                    </div>
                    <div className="mt-1">2) Add this MCP server config:</div>
                  </>
                ) : runtime === "opencode" ? (
                  <>
                    <div className="mt-1">
                      1) Create/edit{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.config/opencode/opencode.json</code>
                    </div>
                    <div className="mt-1">2) Add this MCP server config:</div>
                  </>
                ) : (
                  <>
                    <div className="mt-1">
                      1) Create/edit{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.copilot/mcp-config.json</code>
                    </div>
                    <div className="mt-1">
                      2) Add this MCP server config (or pass it via{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>--additional-mcp-config</code>):
                    </div>
                  </>
                )}
                {runtime !== "custom" ? (
                  <pre
                    className={`mt-1.5 p-2 rounded overflow-x-auto whitespace-pre ${
                      isDark ? "bg-amber-900/20 text-amber-100" : "bg-amber-50 text-amber-900"
                    }`}
                  >
                    <code>
                      {runtime === "opencode" ? OPENCODE_MCP_CONFIG_SNIPPET : runtime === "copilot" ? COPILOT_MCP_CONFIG_SNIPPET : BASIC_MCP_CONFIG_SNIPPET}
                    </code>
                  </pre>
                ) : null}
                <div className={`mt-1 text-[10px] ${isDark ? "text-amber-200/80" : "text-amber-800/80"}`}>
                  Restart the runtime after updating this config.
                </div>
              </div>
            )}
          </div>

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Command</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm font-mono min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={command}
              onChange={(e) => onChangeCommand(e.target.value)}
              placeholder={defaultCommand || "Enter command..."}
            />
            {defaultCommand.trim() ? (
              <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                Default:{" "}
                <code className={`px-1 rounded ${isDark ? "bg-slate-800" : "bg-gray-100"}`}>{defaultCommand}</code>
              </div>
            ) : null}
          </div>
          <div className="flex gap-3 pt-2">
            <button
              className="flex-1 rounded-xl bg-blue-600 hover:bg-blue-500 text-white px-4 py-2.5 text-sm font-semibold shadow-lg disabled:opacity-50 transition-all min-h-[44px]"
              onClick={onSave}
              disabled={busy === "actor-update" || (requireCommand && !command.trim())}
            >
              Save
            </button>
            <button
              className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              }`}
              onClick={onCancel}
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
