// SetupChecklist - 显示设置引导步骤
import { classNames } from "../../utils/classNames";

export interface SetupChecklistProps {
  isDark: boolean;
  selectedGroupId: string;
  busy: string;
  needsScope: boolean;
  needsActors: boolean;
  needsStart: boolean;
  onAddAgent: () => void;
  onStartGroup: () => void;
  /** 紧凑模式用于消息列表上方 */
  variant?: "compact" | "full";
}

// 复制命令到剪贴板
async function copyCommand(cmd: string) {
  try {
    await navigator.clipboard.writeText(cmd);
  } catch {
    window.prompt("Copy command:", cmd);
  }
}

export function SetupChecklist({
  isDark,
  selectedGroupId,
  busy,
  needsScope,
  needsActors,
  needsStart,
  onAddAgent,
  onStartGroup,
  variant = "compact",
}: SetupChecklistProps) {
  const isCompact = variant === "compact";
  const attachCmd = `cccc attach . --group ${selectedGroupId}`;

  // 无需显示任何步骤
  if (!needsScope && !needsActors && !needsStart) {
    if (!isCompact) {
      return (
        <div className={classNames("text-xs mt-4", isDark ? "text-slate-400" : "text-gray-500")}>
          Team is ready. Say hi to your agents.
        </div>
      );
    }
    return null;
  }

  return (
    <div className={classNames("space-y-2", isCompact ? "mt-3" : "mt-4")}>
      {/* Attach Scope */}
      {needsScope && (
        <div
          className={classNames(
            "rounded-xl border px-3 py-2",
            isCompact
              ? isDark ? "border-slate-700 bg-slate-900/60" : "border-gray-200 bg-white"
              : isDark ? "rounded-2xl border-slate-700/50 bg-slate-900/40 p-4 text-left" : "rounded-2xl border-gray-200 bg-white/70 p-4 text-left"
          )}
        >
          <div className={classNames("text-xs font-medium", isDark ? "text-slate-200" : "text-gray-800")}>
            {isCompact ? "Attach a project folder" : ""}
          </div>
          {!isCompact && <div className="text-xs font-semibold">Attach a project folder</div>}
          <div className={classNames(
            "flex items-center justify-between gap-2 text-[11px]",
            isCompact ? "mt-1" : "mt-2",
            isDark ? "text-slate-500" : "text-gray-500"
          )}>
            <code className={classNames("truncate", isDark ? "text-slate-300" : "text-gray-700")}>
              {attachCmd}
            </code>
            <button
              type="button"
              className={classNames(
                "flex-shrink-0 rounded-lg px-2 py-1 text-[11px] font-medium border",
                isDark ? "border-slate-700 text-slate-300 hover:bg-slate-800" : "border-gray-200 text-gray-700 hover:bg-gray-50"
              )}
              onClick={() => copyCommand(attachCmd)}
            >
              Copy
            </button>
          </div>
        </div>
      )}

      {/* Add Agent */}
      {needsActors && (
        <div
          className={classNames(
            isCompact
              ? "flex items-center justify-between gap-3 rounded-xl border px-3 py-2"
              : "rounded-2xl border p-4 text-left",
            isDark
              ? isCompact ? "border-slate-700 bg-slate-900/60" : "border-slate-700/50 bg-slate-900/40"
              : isCompact ? "border-gray-200 bg-white" : "border-gray-200 bg-white/70"
          )}
        >
          <div className={isCompact ? "min-w-0" : ""}>
            <div className={classNames("text-xs font-medium", isDark ? "text-slate-200" : "text-gray-800")}>
              {isCompact ? "Add an agent" : ""}
            </div>
            {!isCompact && <div className="text-xs font-semibold">Add an agent</div>}
            <div className={classNames(
              isCompact ? "text-[11px] truncate" : "mt-1 text-[11px]",
              isDark ? "text-slate-500" : "text-gray-500"
            )}>
              Add a foreman first, then peers.
            </div>
          </div>
          <button
            type="button"
            className={classNames(
              "flex-shrink-0 font-semibold bg-blue-600 hover:bg-blue-500 text-white",
              isCompact ? "rounded-xl px-3 py-1.5 text-[11px]" : "mt-3 w-full rounded-xl px-4 py-2 text-sm"
            )}
            onClick={onAddAgent}
          >
            Add Agent
          </button>
        </div>
      )}

      {/* Start Group */}
      {needsStart && (
        <div
          className={classNames(
            isCompact
              ? "flex items-center justify-between gap-3 rounded-xl border px-3 py-2"
              : "rounded-2xl border p-4 text-left",
            isDark
              ? isCompact ? "border-slate-700 bg-slate-900/60" : "border-slate-700/50 bg-slate-900/40"
              : isCompact ? "border-gray-200 bg-white" : "border-gray-200 bg-white/70"
          )}
        >
          <div className={isCompact ? "min-w-0" : ""}>
            <div className={classNames("text-xs font-medium", isDark ? "text-slate-200" : "text-gray-800")}>
              {isCompact ? "Start the group" : ""}
            </div>
            {!isCompact && <div className="text-xs font-semibold">Start the group</div>}
            <div className={classNames(
              isCompact ? "text-[11px] truncate" : "mt-1 text-[11px]",
              isDark ? "text-slate-500" : "text-gray-500"
            )}>
              Launch your agents and begin chatting.
            </div>
          </div>
          <button
            type="button"
            className={classNames(
              "flex-shrink-0 font-semibold bg-emerald-600 hover:bg-emerald-500 text-white",
              isCompact ? "rounded-xl px-3 py-1.5 text-[11px]" : "mt-3 w-full rounded-xl px-4 py-2 text-sm",
              busy === "group-start" ? "opacity-60" : ""
            )}
            onClick={onStartGroup}
            disabled={busy === "group-start"}
          >
            {busy === "group-start" ? "Starting…" : isCompact ? "Start" : "Start Group"}
          </button>
        </div>
      )}
    </div>
  );
}
