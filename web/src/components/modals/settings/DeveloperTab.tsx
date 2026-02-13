// DeveloperTab configures developer mode.
import { useTranslation } from "react-i18next";
import { inputClass, labelClass, primaryButtonClass, cardClass, preClass } from "./types";

interface DeveloperTabProps {
  isDark: boolean;
  groupId?: string;
  developerMode: boolean;
  setDeveloperMode: (v: boolean) => void;
  logLevel: "INFO" | "DEBUG";
  setLogLevel: (v: "INFO" | "DEBUG") => void;
  terminalBacklogMiB: number;
  setTerminalBacklogMiB: (v: number) => void;
  terminalScrollbackLines: number;
  setTerminalScrollbackLines: (v: number) => void;
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
  // Registry maintenance
  registryBusy: boolean;
  registryErr: string;
  registryResult: {
    dry_run: boolean;
    scanned_groups: number;
    missing_group_ids: string[];
    corrupt_group_ids: string[];
    removed_group_ids: string[];
    removed_default_scope_keys: string[];
  } | null;
  onPreviewRegistry: () => void;
  onReconcileRegistry: () => void;
}

export function DeveloperTab({
  isDark,
  groupId,
  developerMode,
  setDeveloperMode,
  logLevel,
  setLogLevel,
  terminalBacklogMiB,
  setTerminalBacklogMiB,
  terminalScrollbackLines,
  setTerminalScrollbackLines,
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
  registryBusy,
  registryErr,
  registryResult,
  onPreviewRegistry,
  onReconcileRegistry,
}: DeveloperTabProps) {
  const { t } = useTranslation("settings");
  const missing = Array.isArray(registryResult?.missing_group_ids) ? registryResult!.missing_group_ids : [];
  const corrupt = Array.isArray(registryResult?.corrupt_group_ids) ? registryResult!.corrupt_group_ids : [];
  const removed = Array.isArray(registryResult?.removed_group_ids) ? registryResult!.removed_group_ids : [];

  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("developer.title")}</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("developer.description")}
        </p>
        <div className={`mt-2 rounded-lg border px-3 py-2 text-[11px] ${
          isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
        }`}>
          <div className="font-medium">{t("developer.warningTitle")}</div>
          <div className="mt-1">
            {t("developer.warningText")}
          </div>
        </div>
      </div>

      {/* Toggle */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("developer.enableDeveloperMode")}</div>
            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              {t("developer.enableHint")}
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
          <label className={labelClass(isDark)}>{t("developer.logLevel")}</label>
          <select
            value={logLevel}
            onChange={(e) => setLogLevel((e.target.value === "DEBUG" ? "DEBUG" : "INFO"))}
            className={inputClass(isDark)}
          >
            <option value="INFO">INFO</option>
            <option value="DEBUG">DEBUG</option>
          </select>
        </div>

        <div className={`mt-4 pt-3 border-t ${isDark ? "border-slate-800" : "border-gray-200"}`}>
          <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>
            {t("developer.terminalBuffers")}
          </div>
          <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
            {t("developer.terminalBuffersHint")}
          </div>

          <div className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2">
            <div>
              <label className={labelClass(isDark)}>{t("developer.ptyBacklog")}</label>
              <input
                type="number"
                value={terminalBacklogMiB}
                min={1}
                max={50}
                onChange={(e) => setTerminalBacklogMiB(Number(e.target.value || 10))}
                className={inputClass(isDark)}
              />
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("developer.ptyBacklogHint")}
              </div>
            </div>
            <div>
              <label className={labelClass(isDark)}>{t("developer.webScrollback")}</label>
              <input
                type="number"
                value={terminalScrollbackLines}
                min={1000}
                max={200000}
                onChange={(e) => setTerminalScrollbackLines(Number(e.target.value || 8000))}
                className={inputClass(isDark)}
              />
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("developer.webScrollbackHint")}
              </div>
            </div>
          </div>
        </div>

        <div className="mt-3 flex gap-2">
          <button
            onClick={onSaveObservability}
            disabled={obsBusy}
            className={primaryButtonClass(obsBusy)}
          >
            {obsBusy ? t("common:saving") : t("developer.saveDeveloperSettings")}
          </button>
        </div>
      </div>

      {/* Registry maintenance */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("developer.registryTitle")}</div>
            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              {t("developer.registryDescription")}
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={onPreviewRegistry}
              disabled={registryBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
              } disabled:opacity-50`}
            >
              {registryBusy ? t("developer.scanning") : t("developer.scan")}
            </button>
            <button
              onClick={onReconcileRegistry}
              disabled={registryBusy || missing.length === 0}
              className={primaryButtonClass(registryBusy || missing.length === 0)}
            >
              {registryBusy ? t("developer.cleaning") : t("developer.cleanMissing")}
            </button>
          </div>
        </div>

        {registryErr ? (
          <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{registryErr}</div>
        ) : null}

        {registryResult ? (
          <div className={`mt-3 rounded-lg border px-3 py-2 text-xs ${
            isDark ? "border-slate-800 bg-slate-900/40 text-slate-300" : "border-gray-200 bg-white text-gray-700"
          }`}>
            <div>
              {t("developer.scanned")}={registryResult.scanned_groups} · {t("developer.missing")}={missing.length} · {t("developer.corrupt")}={corrupt.length}
              {removed.length > 0 ? ` · ${t("developer.removed")}=${removed.length}` : ""}
            </div>
            {missing.length > 0 ? (
              <div className="mt-2 break-all">
                <span className={isDark ? "text-amber-300" : "text-amber-700"}>{t("developer.missing")}:</span>{" "}
                {missing.join(", ")}
              </div>
            ) : null}
            {corrupt.length > 0 ? (
              <div className="mt-2 break-all">
                <span className={isDark ? "text-rose-300" : "text-rose-700"}>{t("developer.corrupt")}:</span>{" "}
                {corrupt.join(", ")}
              </div>
            ) : null}
            {removed.length > 0 ? (
              <div className="mt-2 break-all">
                <span className={isDark ? "text-emerald-300" : "text-emerald-700"}>{t("developer.removed")}:</span>{" "}
                {removed.join(", ")}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>

      {/* Debug Snapshot */}
      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("developer.debugSnapshot")}</div>
            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              {t("developer.debugSnapshotHint")}
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
              {debugSnapshotBusy ? t("common:loading") : t("developer.refresh")}
            </button>
            <button
              onClick={onClearDebugSnapshot}
              disabled={debugSnapshotBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              } disabled:opacity-50`}
            >
              {t("developer.clear")}
            </button>
          </div>
        </div>

        {!groupId && (
          <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-600"}`}>
            {t("developer.openFromGroup")}
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
            <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("developer.logTail")}</div>
            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
              {t("developer.logTailHint")}
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
              {logBusy ? t("common:loading") : t("developer.refresh")}
            </button>
            <button
              onClick={onClearLogs}
              disabled={!developerMode || logBusy}
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              } disabled:opacity-50`}
            >
              {t("developer.clearTruncate")}
            </button>
          </div>
        </div>

        <div className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2">
          <div>
            <label className={labelClass(isDark)}>{t("developer.component")}</label>
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
            <label className={labelClass(isDark)}>{t("developer.lines")}</label>
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
            {t("developer.imLogsRequireGroup")}
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
