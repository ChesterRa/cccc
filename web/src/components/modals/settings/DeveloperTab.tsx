// DeveloperTab configures developer mode.
import { useTranslation } from "react-i18next";
import { SelectCombobox } from "../../SelectCombobox";
import {
  inputClass,
  labelClass,
  preClass,
  primaryButtonClass,
  settingsWorkspaceActionBarClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";
import type { RuntimeVisibilityMode } from "../../../utils/runtimeVisibility";

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
  peerRuntimeVisibility: RuntimeVisibilityMode;
  setPeerRuntimeVisibility: (v: RuntimeVisibilityMode) => void;
  petRuntimeVisibility: RuntimeVisibilityMode;
  setPetRuntimeVisibility: (v: RuntimeVisibilityMode) => void;
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
  peerRuntimeVisibility,
  setPeerRuntimeVisibility,
  petRuntimeVisibility,
  setPetRuntimeVisibility,
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
  const versionMismatch = Boolean(runtimeVersion && daemonVersion && runtimeVersion !== daemonVersion);

  return (
    <div className="space-y-5">
      <div className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.title")}</h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("developer.description")}
            </p>
            <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/15 px-3 py-2 text-[11px] text-amber-600 dark:text-amber-400">
              <div className="font-medium">{t("developer.warningTitle")}</div>
              <div className="mt-1">
                {t("developer.warningText")}
              </div>
            </div>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className={settingsWorkspacePanelClass(_isDark)}>
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

            <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                <div className="text-[11px] font-medium uppercase tracking-wide text-[var(--color-text-muted)]">
                  {t("developer.ccccVersion")}
                </div>
                <div className="mt-1 text-sm font-semibold text-[var(--color-text-primary)]">
                  {runtimeVersion || "—"}
                </div>
              </div>
              <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
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

          <div className={settingsWorkspacePanelClass(_isDark)}>
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("developer.enableDeveloperMode")}</div>
                <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
                  {t("developer.enableHint")}
                </div>
              </div>
              <label className="inline-flex cursor-pointer items-center">
                <input
                  type="checkbox"
                  className="sr-only"
                  checked={developerMode}
                  onChange={(e) => setDeveloperMode(e.target.checked)}
                />
                <div className={`h-6 w-11 rounded-full transition-colors ${
                  developerMode
                    ? "bg-emerald-500"
                    : "bg-gray-300 dark:bg-slate-700"
                }`}>
                  <div className={`mt-0.5 h-5 w-5 rounded-full bg-white shadow transition-transform ${
                    developerMode ? "translate-x-5" : "translate-x-0.5"
                  }`} />
                </div>
              </label>
            </div>

            <div className="mt-4 space-y-4">
              <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                <label className={labelClass()}>{t("developer.logLevel")}</label>
                <SelectCombobox
                  items={[
                    { value: "INFO", label: "INFO" },
                    { value: "DEBUG", label: "DEBUG" },
                  ]}
                  value={logLevel}
                  onChange={(value) => setLogLevel((value === "DEBUG" ? "DEBUG" : "INFO"))}
                  ariaLabel={t("developer.logLevel")}
                  className={inputClass()}
                />
              </div>

              <div className="pt-3 border-t border-[var(--glass-border-subtle)]">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {t("developer.runtimeVisibilityTitle")}
                </div>
                <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
                  {t("developer.runtimeVisibilityHint")}
                </div>

                <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <label className={labelClass()}>{t("developer.peerRuntime")}</label>
                    <SelectCombobox
                      items={[
                        { value: "visible", label: t("developer.visible") },
                        { value: "hidden", label: t("developer.hidden") },
                      ]}
                      value={peerRuntimeVisibility}
                      onChange={(value) => setPeerRuntimeVisibility(value === "hidden" ? "hidden" : "visible")}
                      ariaLabel={t("developer.peerRuntime")}
                      className={inputClass()}
                    />
                    <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
                      {t("developer.peerRuntimeHint")}
                    </div>
                  </div>
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <label className={labelClass()}>{t("developer.petRuntime")}</label>
                    <SelectCombobox
                      items={[
                        { value: "hidden", label: t("developer.hidden") },
                        { value: "visible", label: t("developer.visible") },
                      ]}
                      value={petRuntimeVisibility}
                      onChange={(value) => setPetRuntimeVisibility(value === "visible" ? "visible" : "hidden")}
                      ariaLabel={t("developer.petRuntime")}
                      className={inputClass()}
                    />
                    <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
                      {t("developer.petRuntimeHint")}
                    </div>
                  </div>
                </div>
              </div>

              <div className="pt-3 border-t border-[var(--glass-border-subtle)]">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {t("developer.terminalBuffers")}
                </div>
                <div className="text-xs mt-0.5 text-[var(--color-text-muted)]">
                  {t("developer.terminalBuffersHint")}
                </div>

                <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
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
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
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
            </div>
          </div>
        </div>
        <div className={settingsWorkspaceActionBarClass(_isDark)}>
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
      <div className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
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
        </div>

        <div className={settingsWorkspaceBodyClass}>
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
      </div>

      {/* Debug Snapshot */}
      <div className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
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
        </div>

        <div className={settingsWorkspaceBodyClass}>
        {!groupId && (
          <div className="mt-2 text-xs text-[var(--color-text-muted)]">
            {t("developer.openFromGroup")}
          </div>
        )}

        {debugSnapshotErr && (
          <div className="mt-2 text-xs text-rose-600 dark:text-rose-400">{debugSnapshotErr}</div>
        )}

        <pre className={`${preClass()} mt-0`}>
          <code>{debugSnapshot || "—"}</code>
        </pre>
        </div>
      </div>

      {/* Log Tail */}
      <div className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
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
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
              <label className={labelClass()}>{t("developer.component")}</label>
              <SelectCombobox
                items={[
                  { value: "daemon", label: "daemon" },
                  { value: "web", label: "web" },
                  { value: "im", label: "im" },
                ]}
                value={logComponent}
                onChange={(value) => setLogComponent((value === "im" ? "im" : value === "web" ? "web" : "daemon"))}
                ariaLabel={t("developer.component")}
                className={inputClass()}
              />
            </div>
            <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
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

        <pre className={`${preClass()} mt-0 max-h-[260px] overflow-y-auto`}>
          <code>{logText || "—"}</code>
        </pre>
        </div>
      </div>
    </div>
  );
}
