import {
  RuntimeInfo,
  SupportedRuntime,
  SUPPORTED_RUNTIMES,
  RUNTIME_INFO,
} from "../../types";
import { BASIC_MCP_CONFIG_SNIPPET, COPILOT_MCP_CONFIG_SNIPPET, OPENCODE_MCP_CONFIG_SNIPPET } from "../../utils/mcpConfigSnippets";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";

export interface AddActorModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  hasForeman: boolean;
  runtimes: RuntimeInfo[];

  suggestedActorId: string;
  newActorId: string;
  setNewActorId: (id: string) => void;

  newActorRole: "peer" | "foreman";
  setNewActorRole: (role: "peer" | "foreman") => void;

  newActorRuntime: SupportedRuntime;
  setNewActorRuntime: (runtime: SupportedRuntime) => void;

  newActorCommand: string;
  setNewActorCommand: (cmd: string) => void;

  newActorSecretsSetText: string;
  setNewActorSecretsSetText: (v: string) => void;

  showAdvancedActor: boolean;
  setShowAdvancedActor: (show: boolean) => void;

  addActorError: string;
  setAddActorError: (msg: string) => void;

  canAddActor: boolean;
  addActorDisabledReason: string;

  onAddActor: () => void;
  onClose: () => void;
  onCancelAndReset: () => void;
}

export function AddActorModal({
  isOpen,
  isDark,
  busy,
  hasForeman,
  runtimes,
  suggestedActorId,
  newActorId,
  setNewActorId,
  newActorRole,
  setNewActorRole,
  newActorRuntime,
  setNewActorRuntime,
  newActorCommand,
  setNewActorCommand,
  newActorSecretsSetText,
  setNewActorSecretsSetText,
  showAdvancedActor,
  setShowAdvancedActor,
  addActorError,
  setAddActorError,
  canAddActor,
  addActorDisabledReason,
  onAddActor,
  onClose,
  onCancelAndReset,
}: AddActorModalProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  if (!isOpen) return null;

  const defaultCommand = runtimes.find((r) => r.name === newActorRuntime)?.recommended_command || "";

  return (
    <div
      className={`fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/50" : "bg-black/30"}`}
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-actor-title"
    >
      <div
        ref={modalRef}
        className={`w-full h-full sm:h-auto sm:max-w-lg sm:mt-16 sm:max-h-[80vh] overflow-y-auto border shadow-2xl animate-scale-in rounded-none sm:rounded-2xl ${
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        }`}
      >
        <div className={`px-6 py-4 border-b sticky top-0 safe-area-inset-top ${isDark ? "border-slate-700/50 bg-slate-800" : "border-gray-200 bg-white"}`}>
          <div id="add-actor-title" className={`text-lg font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
            Add AI Agent
          </div>
          <div className={`text-sm mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Choose an AI runtime to add to your team</div>
        </div>
        <div className="p-6 space-y-5">
          {addActorError && (
            <div
              className={`rounded-xl border px-4 py-2.5 text-sm flex items-center justify-between gap-3 ${
                isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
              }`}
              role="alert"
            >
              <span>{addActorError}</span>
              <button
                className={isDark ? "text-rose-300 hover:text-rose-100" : "text-rose-500 hover:text-rose-700"}
                onClick={() => setAddActorError("")}
              >
                ×
              </button>
            </div>
          )}

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
              Agent Name <span className={isDark ? "text-slate-500" : "text-gray-400"}>(supports Unicode, incl. CJK)</span>
            </label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark
                  ? "bg-slate-900/80 border-slate-600/50 text-white placeholder-slate-500 focus:border-blue-500"
                  : "bg-white border-gray-300 text-gray-900 placeholder-gray-400 focus:border-blue-500"
              }`}
              value={newActorId}
              onChange={(e) => setNewActorId(e.target.value)}
              placeholder={suggestedActorId}
            />
            <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              Leave empty to use:{" "}
              <code className={`px-1 rounded ${isDark ? "bg-slate-800" : "bg-gray-100"}`}>{suggestedActorId}</code>
            </div>
          </div>

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>AI Runtime</label>
            <select
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={newActorRuntime}
              onChange={(e) => {
                const next = e.target.value as SupportedRuntime;
                setNewActorRuntime(next);
                setNewActorCommand("");
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
            {RUNTIME_INFO[newActorRuntime]?.desc ? (
              <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{RUNTIME_INFO[newActorRuntime].desc}</div>
            ) : null}

            {(newActorRuntime === "cursor" ||
              newActorRuntime === "kilocode" ||
              newActorRuntime === "opencode" ||
              newActorRuntime === "copilot" ||
              newActorRuntime === "custom") && (
              <div
                className={`mt-2 rounded-xl border px-3 py-2 text-[11px] ${
                  isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
                }`}
              >
                <div className="font-medium">Manual MCP install required</div>
                {newActorRuntime === "custom" ? (
                  <>
                    <div className="mt-1">
                      Provide a command for your runtime in <span className="font-medium">Advanced options → Command Override</span>.
                    </div>
                    <div className="mt-1">
                      Configure your runtime to add an MCP stdio server named{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc</code> that runs{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc mcp</code>.
                    </div>
                  </>
                ) : newActorRuntime === "cursor" ? (
                  <>
                    <div className="mt-1">
                      1) Create/edit{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.cursor/mcp.json</code> (or{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.cursor/mcp.json</code> in this project)
                    </div>
                    <div className="mt-1">2) Add this MCP server config:</div>
                  </>
                ) : newActorRuntime === "kilocode" ? (
                  <>
                    <div className="mt-1">
                      1) Create/edit{" "}
                      <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.kilocode/mcp.json</code> in this project root
                    </div>
                    <div className="mt-1">2) Add this MCP server config:</div>
                  </>
                ) : newActorRuntime === "opencode" ? (
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

                {newActorRuntime !== "custom" ? (
                  <pre className={`mt-1.5 p-2 rounded overflow-x-auto whitespace-pre ${isDark ? "bg-amber-900/20 text-amber-100" : "bg-amber-50 text-amber-900"}`}>
                    <code>
                      {newActorRuntime === "opencode"
                        ? OPENCODE_MCP_CONFIG_SNIPPET
                        : newActorRuntime === "copilot"
                          ? COPILOT_MCP_CONFIG_SNIPPET
                          : BASIC_MCP_CONFIG_SNIPPET}
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
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Role</label>
            <div className="flex gap-2">
              <button
                className={classNames(
                  "flex-1 px-4 py-2.5 rounded-xl border text-sm font-medium transition-all min-h-[44px]",
                  newActorRole === "foreman"
                    ? "bg-amber-500/20 border-amber-500 text-amber-600"
                    : hasForeman
                      ? isDark
                        ? "bg-slate-900/30 border-slate-700/30 text-slate-500 cursor-not-allowed"
                        : "bg-gray-100 border-gray-200 text-gray-400 cursor-not-allowed"
                      : isDark
                        ? "bg-slate-800/50 border-slate-600/50 text-slate-300 hover:border-slate-500"
                        : "bg-gray-50 border-gray-200 text-gray-600 hover:border-gray-300"
                )}
                onClick={() => {
                  if (!hasForeman) setNewActorRole("foreman");
                }}
                disabled={hasForeman}
              >
                ★ Foreman {hasForeman && "(exists)"}
              </button>
              <button
                className={classNames(
                  "flex-1 px-4 py-2.5 rounded-xl border text-sm font-medium transition-all min-h-[44px]",
                  newActorRole === "peer"
                    ? "bg-blue-500/20 border-blue-500 text-blue-600"
                    : !hasForeman
                      ? isDark
                        ? "bg-slate-900/30 border-slate-700/30 text-slate-500 cursor-not-allowed"
                        : "bg-gray-100 border-gray-200 text-gray-400 cursor-not-allowed"
                      : isDark
                        ? "bg-slate-800/50 border-slate-600/50 text-slate-300 hover:border-slate-500"
                        : "bg-gray-50 border-gray-200 text-gray-600 hover:border-gray-300"
                )}
                onClick={() => {
                  if (hasForeman) setNewActorRole("peer");
                }}
                disabled={!hasForeman}
              >
                Peer {!hasForeman && "(need foreman first)"}
              </button>
            </div>
            <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {hasForeman ? "Foreman leads the team. Peers are worker agents." : "First agent must be the foreman (team leader)."}
            </div>
          </div>

          <button
            className={`flex items-center gap-2 text-xs min-h-[36px] ${isDark ? "text-slate-400 hover:text-slate-300" : "text-gray-500 hover:text-gray-700"}`}
            onClick={() => setShowAdvancedActor(!showAdvancedActor)}
          >
            <span className={classNames("transition-transform", showAdvancedActor && "rotate-90")}>▶</span>
            Advanced options
          </button>

          {showAdvancedActor && (
            <div className={`space-y-4 pl-4 border-l-2 ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
              <div>
                <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Command Override</label>
                <input
                  className={`w-full rounded-xl border px-3 py-2 text-sm font-mono min-h-[44px] transition-colors ${
                    isDark
                      ? "bg-slate-900/80 border-slate-600/50 text-white placeholder-slate-500 focus:border-blue-500"
                      : "bg-white border-gray-300 text-gray-900 placeholder-gray-400 focus:border-blue-500"
                  }`}
                  value={newActorCommand}
                  onChange={(e) => setNewActorCommand(e.target.value)}
                  placeholder={defaultCommand || "Enter command..."}
                />
                {defaultCommand.trim() ? (
                  <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                    Default:{" "}
                    <code className={`px-1 rounded ${isDark ? "bg-slate-800" : "bg-gray-100"}`}>{defaultCommand}</code>
                  </div>
                ) : null}
              </div>

              <div>
                <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Secrets (write-only)</label>
                <textarea
                  className={`w-full rounded-xl border px-3 py-2 text-sm font-mono min-h-[96px] transition-colors ${
                    isDark
                      ? "bg-slate-900/80 border-slate-600/50 text-white placeholder-slate-500 focus:border-blue-500"
                      : "bg-white border-gray-300 text-gray-900 placeholder-gray-400 focus:border-blue-500"
                  }`}
                  value={newActorSecretsSetText}
                  onChange={(e) => setNewActorSecretsSetText(e.target.value)}
                  placeholder={'export OPENAI_API_KEY="...";\nexport ANTHROPIC_API_KEY="...";'}
                />
                <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  Stored locally under{" "}
                  <code className={`px-1 rounded ${isDark ? "bg-slate-800" : "bg-gray-100"}`}>CCCC_HOME/state/…</code> (not in group ledger). Values are never shown again.
                </div>
                <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  Supports <code>KEY=VALUE</code> and shell-style <code>export KEY=VALUE</code> (quotes / semicolons / inline comments).
                </div>
              </div>
            </div>
          )}

          <div className="flex gap-3 pt-2">
            <div className="flex-1">
              <button
                className="w-full rounded-xl bg-blue-600 hover:bg-blue-500 text-white px-4 py-2.5 text-sm font-semibold shadow-lg disabled:opacity-50 disabled:cursor-not-allowed transition-all min-h-[44px]"
                onClick={onAddActor}
                disabled={!canAddActor}
              >
                {busy === "actor-add" ? "Adding..." : "Add Agent"}
              </button>
              {addActorDisabledReason && <div className="text-[10px] text-amber-500 mt-1.5">{addActorDisabledReason}</div>}
            </div>
            <button
              className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              }`}
              onClick={onCancelAndReset}
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
