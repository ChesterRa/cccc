// DeveloperTab - 开发者模式设置
import { inputClass, labelClass, primaryButtonClass, cardClass, preClass } from "./types";

interface DeveloperTabProps {
  isDark: boolean;
  groupId?: string;
  developerMode: boolean;
  setDeveloperMode: (v: boolean) => void;
  logLevel: "INFO" | "DEBUG";
  setLogLevel: (v: "INFO" | "DEBUG") => void;
  obsBusy: boolean;
  onSaveObservability: () => void;
  // Debug snapshot
  debugSnapshot: string;
  debugSnapshotErr: string;
  debugSnapshotBusy: boolean;
  onLoadDebugSnapshot: () => void;
  onClearDebugSnapshot: () => void;
  // Log tail
  logComponent: "daemon" | "web" | "im";
  setLogComponent: (v: "daemon" | "web" | "im") => void;
  logLines: number;
  setLogLines: (v: number) => void;
  logText: string;
  logErr: string;
  logBusy: boolean;
  onLoadLogTail: () => void;
  onClearLogs: () => void;
}

export function DeveloperTab({
  isDark,
  groupId,
  developerMode,
  setDeveloperMode,
  logLevel,
  setLogLevel,
  obsBusy,
  onSaveObservability,
  debugSnapshot,
  debugSnapshotErr,
  debugSnapshotBusy,
  onLoadDebugSnapshot,
  onClearDebugSnapshot,
  logComponent,
  setLogComponent,
  logLines,
  setLogLines,
  logText,
  logErr,
  logBusy,
  onLoadLogTail,
  onClearLogs,
}: DeveloperTabProps) {
  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Developer Mode (Global)</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          These settings apply to the whole CCCC instance (daemon + Web). Use this only when debugging.
        </p>
        <div className={`mt-2 rounded-lg border px-3 py-2 text-[11px] ${
          isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
        }`}>
          <div className="font-medium">Warning</div>
          <div className="mt-1">
            Developer mode enables verbose logs and extra diagnostics. Only enable it when you need it.
          </div>
        </div>
      </div>

      {/* Toggle */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>Enable developer mode</div>
            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              Enables debug snapshot and log tail tools.
            </div>
          </div>
          <label className="inline-flex items-center cursor-pointer">
            <input
              type="checkbox"
              className="sr-only"
              checked={developerMode}
              onChange={(e) => setDeveloperMode(e.target.checked)}
            />
            <div className={`w-11 h-6 rounded-full transition-colors ${
              developerMode
                ? (isDark ? "bg-emerald-600" : "bg-emerald-500")
                : (isDark ? "bg-slate-700" : "bg-gray-300")
            }`}>
              <div className={`w-5 h-5 bg-white rounded-full shadow transform transition-transform mt-0.5 ${
                developerMode ? "translate-x-5" : "translate-x-0.5"
              }`} />
            </div>
          </label>
        </div>

        <div className="mt-3">
          <label className={labelClass(isDark)}>Log level</label>
          <select
            value={logLevel}
            onChange={(e) => setLogLevel((e.target.value === "DEBUG" ? "DEBUG" : "INFO"))}
            className={inputClass(isDark)}
          >
            <option value="INFO">INFO</option>
            <option value="DEBUG">DEBUG</option>
          </select>
        </div>

        <div className="mt-3 flex gap-2">
          <button
            onClick={onSaveObservability}
            disabled={obsBusy}
            className={primaryButtonClass(obsBusy)}
          >
            {obsBusy ? "Saving..." : "Save Developer Settings"}
          </button>
        </div>
      </div>

      {/* Debug Snapshot */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>Debug snapshot (this group)</div>
            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              Shows daemon/actors state + delivery throttle summary (developer mode only).
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={onLoadDebugSnapshot}
              disabled={!developerMode || !groupId || debugSnapshotBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              } disabled:opacity-50`}
            >
              {debugSnapshotBusy ? "Loading..." : "Refresh"}
            </button>
            <button
              onClick={onClearDebugSnapshot}
              disabled={debugSnapshotBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              } disabled:opacity-50`}
            >
              Clear
            </button>
          </div>
        </div>

        {!groupId && (
          <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-600"}`}>
            Open Settings from an active group to view group-scoped debug info.
          </div>
        )}

        {debugSnapshotErr && (
          <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{debugSnapshotErr}</div>
        )}

        <pre className={preClass(isDark)}>
          <code>{debugSnapshot || "—"}</code>
        </pre>
      </div>

      {/* Log Tail */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>Log tail</div>
            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              Tails local log files (developer mode only). Component "im" is group-scoped.
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={onLoadLogTail}
              disabled={!developerMode || logBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              } disabled:opacity-50`}
            >
              {logBusy ? "Loading..." : "Refresh"}
            </button>
            <button
              onClick={onClearLogs}
              disabled={!developerMode || logBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              } disabled:opacity-50`}
            >
              Clear (truncate)
            </button>
          </div>
        </div>

        <div className="mt-3 grid grid-cols-2 gap-2">
          <div>
            <label className={labelClass(isDark)}>Component</label>
            <select
              value={logComponent}
              onChange={(e) => setLogComponent((e.target.value === "im" ? "im" : e.target.value === "web" ? "web" : "daemon"))}
              className={inputClass(isDark)}
            >
              <option value="daemon">daemon</option>
              <option value="web">web</option>
              <option value="im">im</option>
            </select>
          </div>
          <div>
            <label className={labelClass(isDark)}>Lines</label>
            <input
              type="number"
              value={logLines}
              min={50}
              max={2000}
              onChange={(e) => setLogLines(Number(e.target.value || 200))}
              className={inputClass(isDark)}
            />
          </div>
        </div>

        {logComponent === "im" && !groupId && (
          <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-600"}`}>
            IM logs require a group_id; open Settings from a group.
          </div>
        )}

        {logErr && (
          <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{logErr}</div>
        )}

        <pre className={`${preClass(isDark)} max-h-[260px] overflow-y-auto`}>
          <code>{logText || "—"}</code>
        </pre>
      </div>
    </div>
  );
}
