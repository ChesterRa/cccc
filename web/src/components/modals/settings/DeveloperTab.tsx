// DeveloperTab configures developer mode and its diagnostics.
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { copyTextToClipboard } from "../../../utils/copy";
import { buildMismatch, type RuntimeBuildInfo } from "../../../utils/runtimeBuildInfo";
import type { RuntimeVisibilityMode } from "../../../utils/runtimeVisibility";
import { SegmentedChoice } from "../../ui/segmented-choice";
import { dangerButtonClass, secondaryButtonClass } from "./types";
import {
  SettingsCard,
  SettingsNumberInput,
  SettingsPage,
  SettingsRow,
  SettingsSaveBar,
  SettingsSwitch,
} from "./SettingsKit";

interface DeveloperTabProps {
  isDark: boolean;
  groupId?: string;
  runtimeVersion: string;
  runtimeBuildInfo?: RuntimeBuildInfo;
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
  assistantRuntimeVisibility: RuntimeVisibilityMode;
  setAssistantRuntimeVisibility: (v: RuntimeVisibilityMode) => void;
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
  groupId,
  runtimeVersion,
  runtimeBuildInfo,
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
  assistantRuntimeVisibility,
  setAssistantRuntimeVisibility,
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
  const [copyStatus, setCopyStatus] = useState<"" | "copied" | "copyFailed">("");
  const missing = Array.isArray(registryResult?.missing_group_ids)
    ? registryResult!.missing_group_ids
    : [];
  const corrupt = Array.isArray(registryResult?.corrupt_group_ids)
    ? registryResult!.corrupt_group_ids
    : [];
  const removed = Array.isArray(registryResult?.removed_group_ids)
    ? registryResult!.removed_group_ids
    : [];
  const versionMismatch = Boolean(
    (runtimeVersion && daemonVersion && runtimeVersion !== daemonVersion) ||
    buildMismatch(runtimeBuildInfo),
  );

  const ids = {
    developerMode: useId(),
    logLevel: useId(),
    peerRuntime: useId(),
    assistantRuntime: useId(),
    ptyBacklog: useId(),
    webScrollback: useId(),
    logComponent: useId(),
    logLines: useId(),
  };
  const ghost = secondaryButtonClass("sm");
  const visibilityOptions = (order: RuntimeVisibilityMode[]) =>
    order.map((value) => ({ value, label: t(`developer.${value}`) }));
  const output = (text: string, extraClass = "") => (
    <pre
      className={`m-0 min-h-[100px] overflow-auto rounded-[10px] bg-zinc-900 p-3.5 font-mono text-xs leading-6 whitespace-pre text-zinc-200 ${extraClass}`}
    >
      <code>{text || "—"}</code>
    </pre>
  );
  const note = (text: string, tone: "muted" | "error" | "warn" = "muted") => (
    <div
      className={
        tone === "error"
          ? "text-xs text-rose-600 dark:text-rose-400"
          : tone === "warn"
            ? "text-xs text-amber-700 dark:text-amber-300"
            : "text-xs text-[var(--color-text-muted)]"
      }
    >
      {text}
    </div>
  );

  return (
    <SettingsPage
      title={t("developer.title")}
      description={
        <>
          {t("developer.description")}{" "}
          <span className="text-amber-700 dark:text-amber-300">
            <strong className="font-medium">{t("developer.warningTitle")}</strong>{" "}
            {t("developer.warningText")}
          </span>
        </>
      }
      footer={
        <SettingsSaveBar
          label={t("developer.saveDeveloperSettings")}
          busy={obsBusy}
          onSave={onSaveObservability}
        />
      }
    >
      <SettingsCard
        title={t("developer.runtimeInfoTitle")}
        description={t("developer.runtimeInfoHint")}
        actions={
          versionMismatch || runtimeBuildInfo ? (
            <>
              {versionMismatch ? (
                <span className="inline-flex items-center rounded-full border border-amber-500/30 bg-amber-500/15 px-2.5 text-xs font-medium text-amber-700 dark:text-amber-300">
                  {t("developer.versionMismatchBadge")}
                </span>
              ) : null}
              {runtimeBuildInfo ? (
                <button
                  type="button"
                  className={ghost}
                  onClick={async () => {
                    const ok = await copyTextToClipboard(
                      JSON.stringify(
                        {
                          web_version: runtimeVersion,
                          daemon_version: daemonVersion,
                          ...runtimeBuildInfo,
                        },
                        null,
                        2,
                      ),
                    );
                    setCopyStatus(ok ? "copied" : "copyFailed");
                  }}
                >
                  {t("developer.copyBuildInfo")}
                </button>
              ) : null}
            </>
          ) : undefined
        }
      >
        <SettingsRow
          label={`${t("developer.ccccVersion")} / ${t("developer.daemonVersion")}`}
          help={versionMismatch ? t("developer.buildMismatchHint") : undefined}
        >
          <span className="inline-flex items-center font-mono text-[13px] text-[var(--color-text-secondary)]">
            {runtimeVersion || "—"} / {daemonVersion || "—"}
          </span>
          <span role="status" className="text-xs text-[var(--color-text-secondary)]">
            {copyStatus ? t(`developer.${copyStatus}`) : ""}
          </span>
        </SettingsRow>
        {runtimeBuildInfo
          ? (["webSource", "daemonSource", "webAssets", "servedEntry", "loadedEntry"] as const).map(
              (key) => (
                <SettingsRow key={key} label={t(`developer.${key}`)}>
                  <span className="inline-flex items-center font-mono text-xs break-all text-[var(--color-text-secondary)]">
                    {runtimeBuildInfo[key] || "—"}
                  </span>
                </SettingsRow>
              ),
            )
          : null}
        {runtimeInfoErr ? <div className="px-5 py-3">{note(runtimeInfoErr, "error")}</div> : null}
      </SettingsCard>

      <SettingsCard>
        <SettingsRow
          label={t("developer.enableDeveloperMode")}
          help={t("developer.enableHint")}
          labelId={ids.developerMode}
        >
          <SettingsSwitch
            checked={developerMode}
            onChange={setDeveloperMode}
            labelledBy={ids.developerMode}
          />
        </SettingsRow>
        <SettingsRow label={t("developer.logLevel")} labelId={ids.logLevel}>
          <SegmentedChoice
            fill={false}
            labelledBy={ids.logLevel}
            value={logLevel}
            options={[
              { value: "INFO", label: "INFO" },
              { value: "DEBUG", label: "DEBUG" },
            ]}
            onChange={setLogLevel}
          />
        </SettingsRow>
      </SettingsCard>

      <SettingsCard
        title={t("developer.runtimeVisibilityTitle")}
        description={t("developer.runtimeVisibilityHint")}
      >
        <SettingsRow
          label={t("developer.peerRuntime")}
          help={t("developer.peerRuntimeHint")}
          labelId={ids.peerRuntime}
        >
          <SegmentedChoice
            fill={false}
            labelledBy={ids.peerRuntime}
            value={peerRuntimeVisibility}
            options={visibilityOptions(["visible", "hidden"])}
            onChange={setPeerRuntimeVisibility}
          />
        </SettingsRow>
        <SettingsRow
          label={t("developer.assistantRuntime")}
          help={t("developer.assistantRuntimeHint")}
          labelId={ids.assistantRuntime}
        >
          <SegmentedChoice
            fill={false}
            labelledBy={ids.assistantRuntime}
            value={assistantRuntimeVisibility}
            options={visibilityOptions(["hidden", "visible"])}
            onChange={setAssistantRuntimeVisibility}
          />
        </SettingsRow>
      </SettingsCard>

      <SettingsCard
        title={t("developer.terminalBuffers")}
        description={t("developer.terminalBuffersHint")}
      >
        <SettingsRow
          label={t("developer.ptyBacklog")}
          help={t("developer.ptyBacklogHint")}
          htmlFor={ids.ptyBacklog}
        >
          <SettingsNumberInput
            id={ids.ptyBacklog}
            value={terminalBacklogMiB}
            min={1}
            max={50}
            onChange={(value) => setTerminalBacklogMiB(value || 10)}
          />
        </SettingsRow>
        <SettingsRow
          label={t("developer.webScrollback")}
          help={t("developer.webScrollbackHint")}
          htmlFor={ids.webScrollback}
        >
          <SettingsNumberInput
            id={ids.webScrollback}
            value={terminalScrollbackLines}
            min={1000}
            max={200000}
            onChange={(value) => setTerminalScrollbackLines(value || 8000)}
          />
        </SettingsRow>
      </SettingsCard>

      <SettingsCard
        title={t("developer.debugSnapshot")}
        description={t("developer.debugSnapshotHint")}
        actions={
          <>
            <button
              type="button"
              onClick={onLoadDebugSnapshot}
              disabled={!developerMode || !groupId || debugSnapshotBusy}
              className={ghost}
            >
              {debugSnapshotBusy ? t("common:loading") : t("developer.refresh")}
            </button>
            <button
              type="button"
              onClick={onClearDebugSnapshot}
              disabled={debugSnapshotBusy}
              className={ghost}
            >
              {t("developer.clear")}
            </button>
          </>
        }
      >
        <div className="flex flex-col gap-2 px-5 py-4">
          {!groupId ? note(t("developer.openFromGroup")) : null}
          {debugSnapshotErr ? note(debugSnapshotErr, "error") : null}
          {output(debugSnapshot)}
        </div>
      </SettingsCard>

      <SettingsCard
        title={t("developer.logTail")}
        description={t("developer.logTailHint")}
        actions={
          <button
            type="button"
            onClick={onLoadLogTail}
            disabled={!developerMode || logBusy}
            className={ghost}
          >
            {logBusy ? t("common:loading") : t("developer.refresh")}
          </button>
        }
      >
        <SettingsRow label={t("developer.component")} labelId={ids.logComponent}>
          <SegmentedChoice
            fill={false}
            labelledBy={ids.logComponent}
            value={logComponent}
            options={[
              { value: "daemon", label: "daemon" },
              { value: "web", label: "web" },
              { value: "im", label: "im" },
            ]}
            onChange={setLogComponent}
          />
        </SettingsRow>
        <SettingsRow label={t("developer.lines")} htmlFor={ids.logLines}>
          <SettingsNumberInput
            id={ids.logLines}
            value={logLines}
            min={50}
            max={2000}
            onChange={(value) => setLogLines(value || 200)}
          />
        </SettingsRow>
        <div className="flex flex-col gap-2 px-5 py-4">
          {logComponent === "im" && !groupId ? note(t("developer.imLogsRequireGroup")) : null}
          {logErr ? note(logErr, "error") : null}
          {output(logText, "max-h-[260px]")}
        </div>
      </SettingsCard>

      <SettingsCard
        tone="danger"
        title={t("developer.clearTruncate")}
        actions={
          <button
            type="button"
            onClick={onClearLogs}
            disabled={!developerMode || logBusy}
            className={dangerButtonClass("sm")}
          >
            {t("developer.clearTruncate")}
          </button>
        }
      />

      <SettingsCard
        tone="danger"
        title={t("developer.registryTitle")}
        description={t("developer.registryDescription")}
        actions={
          <>
            <button
              type="button"
              onClick={onPreviewRegistry}
              disabled={registryBusy}
              className={ghost}
            >
              {registryBusy ? t("developer.scanning") : t("developer.scan")}
            </button>
            <button
              type="button"
              onClick={onReconcileRegistry}
              disabled={registryBusy || missing.length === 0}
              className={dangerButtonClass("sm")}
            >
              {registryBusy ? t("developer.cleaning") : t("developer.cleanMissing")}
            </button>
          </>
        }
      >
        {registryErr || registryResult ? (
          <div className="flex flex-col gap-2 px-5 py-4 text-xs text-[var(--color-text-secondary)]">
            {registryErr ? note(registryErr, "error") : null}
            {registryResult ? (
              <>
                <div>
                  {t("developer.scanned")}={registryResult.scanned_groups} ·{" "}
                  {t("developer.missing")}={missing.length} · {t("developer.corrupt")}=
                  {corrupt.length}
                  {removed.length > 0 ? ` · ${t("developer.removed")}=${removed.length}` : ""}
                </div>
                {missing.length > 0 ? (
                  <div className="break-all">
                    <span className="text-amber-600 dark:text-amber-400">
                      {t("developer.missing")}:
                    </span>{" "}
                    {missing.join(", ")}
                  </div>
                ) : null}
                {corrupt.length > 0 ? (
                  <div className="break-all">
                    <span className="text-rose-600 dark:text-rose-400">
                      {t("developer.corrupt")}:
                    </span>{" "}
                    {corrupt.join(", ")}
                  </div>
                ) : null}
                {removed.length > 0 ? (
                  <div className="break-all">
                    <span className="text-emerald-600 dark:text-emerald-400">
                      {t("developer.removed")}:
                    </span>{" "}
                    {removed.join(", ")}
                  </div>
                ) : null}
              </>
            ) : null}
          </div>
        ) : null}
      </SettingsCard>
    </SettingsPage>
  );
}
