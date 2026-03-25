// DeveloperTab configures developer mode.
import { useTranslation } from "react-i18next";
import { inputClass, labelClass, primaryButtonClass, cardClass, preClass } from "./types";

interface DeveloperTabProps {
  isDark: boolean;
  groupId?: string;
  runtimeVersion: string;
  daemonVersion: string;
  runtimeInfoErr: string;
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
  feedbackBundleBusy: boolean;
  feedbackBundleErr: string;
  feedbackBundleInfo: string;
  onExportFeedbackBundle: () => void;
}

export function DeveloperTab({
  isDark: _isDark,
  groupId,
  runtimeVersion,
  daemonVersion,
  runtimeInfoErr,
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
  feedbackBundleBusy,
  feedbackBundleErr,
  feedbackBundleInfo,
  onExportFeedbackBundle,
}: DeveloperTabProps) {
  const { t } = useTranslation("settings");
  const missing = Array.isArray(registryResult?.missing_group_ids) ? registryResult!.missing_group_ids : [];
  const corrupt = Array.isArray(registryResult?.corrupt_group_ids) ? registryResult!.corrupt_group_ids : [];
  const removed = Array.isArray(registryResult?.removed_group_ids) ? registryResult!.removed_group_ids : [];
  const versionMismatch = Boolean(runtimeVersion && daemonVersion && runtimeVersion !== daemonVersion);

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-medium text-[var(--color-text-secondary)]">{t("developer.title")}</h3>
        <p className="text-xs mt-1 text-[var(--color-text-muted)]">
          {t("developer.description")}
        </p>
        <div className="mt-2 rounded-lg border px-3 py-2 text-[11px] border-amber-500/30 bg-amber-500/15 text-amber-600 dark:text-amber-400">
          <div className="font-medium">{t("developer.warningTitle")}</div>
          <div className="mt-1">
            {t("developer.warningText")}
          </div>
        </div>
      </div>

      <div className={cardClass()}>
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.runtimeInfoTitle")}</div>
            <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
              {t("developer.runtimeInfoHint")}
            </div>
          </div>
          {versionMismatch ? (
            <span className="rounded-full border border-amber-500/30 bg-amber-500/15 px-2.5 py-1 text-[11px] font-medium text-amber-700 dark:text-amber-300">
              {t("developer.versionMismatchBadge")}
            </span>
          ) : null}
        </div>

        {runtimeInfoErr ? (
          <div className="mt-2 text-xs text-rose-600 dark:text-rose-400">{runtimeInfoErr}</div>
        ) : null}

        <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
          <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-3 py-2.5">
            <div className="text-[11px] font-medium uppercase tracking-wide text-[var(--color-text-muted)]">
              {t("developer.ccccVersion")}
            </div>
            <div className="mt-1 text-sm font-semibold text-[var(--color-text-primary)]">
              {runtimeVersion || "—"}
            </div>
          </div>
          <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-3 py-2.5">
            <div className="text-[11px] font-medium uppercase tracking-wide text-[var(--color-text-muted)]">
              {t("developer.daemonVersion")}
            </div>
            <div className="mt-1 text-sm font-semibold text-[var(--color-text-primary)]">
              {daemonVersion || "—"}
            </div>
          </div>
        </div>

        {versionMismatch ? (
          <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
            {t("developer.versionMismatchHint")}
          </div>
        ) : null}
      </div>

      {/* Toggle */}
      <div className={cardClass()}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.enableDeveloperMode")}</div>
            <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
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
                ? "bg-emerald-500"
                : "bg-gray-300 dark:bg-slate-700"
            }`}>
              <div className={`w-5 h-5 bg-white rounded-full shadow transform transition-transform mt-0.5 ${
                developerMode ? "translate-x-5" : "translate-x-0.5"
              }`} />
            </div>
          </label>
        </div>

        <div className="mt-3">
          <label className={labelClass()}>{t("developer.logLevel")}</label>
          <select
            value={logLevel}
            onChange={(e) => setLogLevel((e.target.value === "DEBUG" ? "DEBUG" : "INFO"))}
            className={inputClass()}
          >
            <option value="INFO">INFO</option>
            <option value="DEBUG">DEBUG</option>
          </select>
        </div>

        <div className="mt-4 pt-3 border-t border-[var(--glass-border-subtle)]">
          <div className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t("developer.terminalBuffers")}
          </div>
          <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
            {t("developer.terminalBuffersHint")}
          </div>

          <div className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2">
            <div>
              <label className={labelClass()}>{t("developer.ptyBacklog")}</label>
              <input
                type="number"
                value={terminalBacklogMiB}
                min={1}
                max={50}
                onChange={(e) => setTerminalBacklogMiB(Number(e.target.value || 10))}
                className={inputClass()}
              />
              <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
                {t("developer.ptyBacklogHint")}
              </div>
            </div>
            <div>
              <label className={labelClass()}>{t("developer.webScrollback")}</label>
              <input
                type="number"
                value={terminalScrollbackLines}
                min={1000}
                max={200000}
                onChange={(e) => setTerminalScrollbackLines(Number(e.target.value || 8000))}
                className={inputClass()}
              />
              <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
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

      <div className={cardClass()}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.feedbackBundleTitle")}</div>
            <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
              {t("developer.feedbackBundleHint")}
            </div>
          </div>
          <button
            onClick={onExportFeedbackBundle}
            disabled={!developerMode || !groupId || feedbackBundleBusy}
            className={primaryButtonClass(!developerMode || !groupId || feedbackBundleBusy)}
          >
            {feedbackBundleBusy ? t("common:loading") : t("developer.feedbackBundleAction")}
          </button>
        </div>

        {!groupId && (
          <div className="mt-2 text-xs text-[var(--color-text-muted)]">
            {t("developer.openFromGroup")}
          </div>
        )}

        {feedbackBundleErr ? (
          <div className="mt-2 text-xs text-rose-600 dark:text-rose-400">{feedbackBundleErr}</div>
        ) : null}
        {feedbackBundleInfo ? (
          <div className="mt-2 text-xs text-emerald-600 dark:text-emerald-400">{feedbackBundleInfo}</div>
        ) : null}
      </div>

      {/* Registry maintenance */}
      <div className={cardClass()}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.registryTitle")}</div>
            <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
              {t("developer.registryDescription")}
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={onPreviewRegistry}
              disabled={registryBusy}
              className="glass-btn px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors text-[var(--color-text-primary)] disabled:opacity-50"
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
          <div className="mt-2 text-xs text-rose-600 dark:text-rose-400">{registryErr}</div>
        ) : null}

        {registryResult ? (
          <div className="mt-3 rounded-lg border px-3 py-2 text-xs border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] text-[var(--color-text-secondary)]">
            <div>
              {t("developer.scanned")}={registryResult.scanned_groups} · {t("developer.missing")}={missing.length} · {t("developer.corrupt")}={corrupt.length}
              {removed.length > 0 ? ` · ${t("developer.removed")}=${removed.length}` : ""}
            </div>
            {missing.length > 0 ? (
              <div className="mt-2 break-all">
                <span className="text-amber-600 dark:text-amber-400">{t("developer.missing")}:</span>{" "}
                {missing.join(", ")}
              </div>
            ) : null}
            {corrupt.length > 0 ? (
              <div className="mt-2 break-all">
                <span className="text-rose-600 dark:text-rose-400">{t("developer.corrupt")}:</span>{" "}
                {corrupt.join(", ")}
              </div>
            ) : null}
            {removed.length > 0 ? (
              <div className="mt-2 break-all">
                <span className="text-emerald-600 dark:text-emerald-400">{t("developer.removed")}:</span>{" "}
                {removed.join(", ")}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>

      {/* Debug Snapshot */}
      <div className={cardClass()}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.debugSnapshot")}</div>
            <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
              {t("developer.debugSnapshotHint")}
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={onLoadDebugSnapshot}
              disabled={!developerMode || !groupId || debugSnapshotBusy}
              className="glass-btn px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors text-[var(--color-text-primary)] disabled:opacity-50"
            >
              {debugSnapshotBusy ? t("common:loading") : t("developer.refresh")}
            </button>
            <button
              onClick={onClearDebugSnapshot}
              disabled={debugSnapshotBusy}
              className="glass-btn px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors text-[var(--color-text-secondary)] disabled:opacity-50"
            >
              {t("developer.clear")}
            </button>
          </div>
        </div>

        {!groupId && (
          <div className="mt-2 text-xs text-[var(--color-text-muted)]">
            {t("developer.openFromGroup")}
          </div>
        )}

        {debugSnapshotErr && (
          <div className="mt-2 text-xs text-rose-600 dark:text-rose-400">{debugSnapshotErr}</div>
        )}

        <pre className={preClass()}>
          <code>{debugSnapshot || "—"}</code>
        </pre>
      </div>

      {/* Log Tail */}
      <div className={cardClass()}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.logTail")}</div>
            <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
              {t("developer.logTailHint")}
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={onLoadLogTail}
              disabled={!developerMode || logBusy}
              className="glass-btn px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors text-[var(--color-text-primary)] disabled:opacity-50"
            >
              {logBusy ? t("common:loading") : t("developer.refresh")}
            </button>
            <button
              onClick={onClearLogs}
              disabled={!developerMode || logBusy}
              className="glass-btn px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors text-[var(--color-text-secondary)] disabled:opacity-50"
            >
              {t("developer.clearTruncate")}
            </button>
          </div>
        </div>

        <div className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2">
          <div>
            <label className={labelClass()}>{t("developer.component")}</label>
            <select
              value={logComponent}
              onChange={(e) => setLogComponent((e.target.value === "im" ? "im" : e.target.value === "web" ? "web" : "daemon"))}
              className={inputClass()}
            >
              <option value="daemon">daemon</option>
              <option value="web">web</option>
              <option value="im">im</option>
            </select>
          </div>
          <div>
            <label className={labelClass()}>{t("developer.lines")}</label>
            <input
              type="number"
              value={logLines}
              min={50}
              max={2000}
              onChange={(e) => setLogLines(Number(e.target.value || 200))}
              className={inputClass()}
            />
          </div>
        </div>

        {logComponent === "im" && !groupId && (
          <div className="mt-2 text-xs text-[var(--color-text-muted)]">
            {t("developer.imLogsRequireGroup")}
          </div>
        )}

        {logErr && (
          <div className="mt-2 text-xs text-rose-600 dark:text-rose-400">{logErr}</div>
        )}

        <pre className={`${preClass()} max-h-[260px] overflow-y-auto`}>
          <code>{logText || "—"}</code>
        </pre>
      </div>
    </div>
  );
}
