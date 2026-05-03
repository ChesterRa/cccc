import React from "react";
import { useTranslation } from "react-i18next";

import type { AssistantServiceModel, AssistantServiceRuntime } from "../../../types";
import { TrashIcon } from "../../Icons";
import { dangerButtonClass, secondaryButtonClass, settingsWorkspaceSoftPanelClass } from "./types";

type AsrModelRole = "final" | "live";

type StatusTone = "on" | "off" | "info";

interface VoiceSecretaryLocalAsrControlsProps {
  isDark: boolean;
  busy: boolean;
  voiceSaveBusy: boolean;
  runtime: AssistantServiceRuntime | undefined;
  runtimeStatus: string;
  runtimeReady: boolean;
  runtimeInstalling: boolean;
  finalModel: AssistantServiceModel | null;
  finalModelId: string;
  finalModelStatus: string;
  finalModelReady: boolean;
  finalModelInstalling: boolean;
  liveModel: AssistantServiceModel | null;
  liveModelId: string;
  liveModelStatus: string;
  liveModelReady: boolean;
  liveModelInstalling: boolean;
  deletingModelId: string;
  onInstallRuntime: () => void;
  onInstallModel: (modelId: string, role: AsrModelRole) => void;
  onDeleteModel: (modelId: string, role: AsrModelRole) => void;
}

function formatModelSize(bytes: number | undefined): string {
  const value = Number(bytes || 0);
  if (!Number.isFinite(value) || value <= 0) return "";
  if (value >= 1024 * 1024 * 1024) return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
  if (value >= 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KiB`;
  return `${Math.round(value)} B`;
}

function StatusPill({ children, tone }: { children: React.ReactNode; tone: StatusTone }) {
  const classes =
    tone === "on"
      ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
      : tone === "off"
        ? "bg-slate-500/12 text-[var(--color-text-muted)]"
        : "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white";
  return (
    <span className={`inline-flex items-center rounded-full px-2.5 py-1 text-[11px] font-medium ${classes}`}>
      {children}
    </span>
  );
}

function modelStatusLabel(
  status: string,
  model: AssistantServiceModel | null,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  if (status !== "downloading") return t("assistants.serviceRuntimeStatusShort", { status });
  return `${t("assistants.serviceRuntimeStatusShort", { status })} ${Math.round(Number(model?.progress_percent || 0))}%`;
}

export function VoiceSecretaryStatusPill({ children, tone }: { children: React.ReactNode; tone: StatusTone }) {
  return <StatusPill tone={tone}>{children}</StatusPill>;
}

function AsrModelCard({
  role,
  model,
  modelId,
  status,
  ready,
  installing,
  busy,
  voiceSaveBusy,
  deleting,
  onInstall,
  onDelete,
}: {
  role: AsrModelRole;
  model: AssistantServiceModel | null;
  modelId: string;
  status: string;
  ready: boolean;
  installing: boolean;
  busy: boolean;
  voiceSaveBusy: boolean;
  deleting: boolean;
  onInstall: (modelId: string, role: AsrModelRole) => void;
  onDelete: (modelId: string, role: AsrModelRole) => void;
}) {
  const { t } = useTranslation(["settings", "common"]);
  const size = formatModelSize(model?.total_size_bytes);
  const titleKey = role === "final" ? "assistants.finalAsrModelTitle" : "assistants.streamingAsrModelTitle";
  const missingKey = role === "final" ? "assistants.finalAsrModelMissing" : "assistants.streamingAsrModelMissing";
  const hintKey = role === "final" ? "assistants.finalAsrModelHint" : "assistants.streamingAsrModelHint";
  const installKey = role === "final" ? "assistants.finalAsrModelInstall" : "assistants.streamingAsrModelInstall";
  const installingKey = role === "final" ? "assistants.finalAsrModelInstalling" : "assistants.streamingAsrModelInstalling";
  const installedKey = role === "final" ? "assistants.finalAsrModelInstalled" : "assistants.streamingAsrModelInstalled";
  const disabled = busy || voiceSaveBusy || deleting || !modelId || installing;

  return (
    <div className="flex flex-wrap items-start justify-between gap-3 rounded-lg border border-emerald-500/20 bg-emerald-500/5 p-3">
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <div className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t(titleKey)}
          </div>
          <StatusPill tone={ready ? "on" : status === "failed" ? "off" : "info"}>
            {modelStatusLabel(status, model, t)}
          </StatusPill>
          {size ? <StatusPill tone="info">{size}</StatusPill> : null}
        </div>
        <p className="mt-1 break-words text-[11px] leading-5 text-[var(--color-text-muted)]">
          {model?.title || modelId || t(missingKey)}
        </p>
        <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
          {t(hintKey)}
        </p>
        {model?.error?.message ? (
          <p className="mt-1 text-[11px] leading-5 text-rose-700 dark:text-rose-300">
            {t("assistants.serviceRuntimeError", { message: String(model.error.message || "") })}
          </p>
        ) : null}
      </div>
      <div className="flex flex-wrap justify-end gap-2">
        <button
          type="button"
          onClick={() => onInstall(modelId, role)}
          disabled={disabled || ready}
          className={secondaryButtonClass("sm")}
        >
          {ready ? t(installedKey) : installing ? t(installingKey) : t(installKey)}
        </button>
        {ready ? (
          <button
            type="button"
            onClick={() => onDelete(modelId, role)}
            disabled={disabled}
            className={dangerButtonClass("sm")}
            title={t("assistants.deleteAsrModel")}
            aria-label={t("assistants.deleteAsrModel")}
          >
            <TrashIcon size={14} />
            {deleting ? t("assistants.deletingAsrModel") : t("assistants.deleteAsrModel")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

export function VoiceSecretaryLocalAsrControls({
  isDark,
  busy,
  voiceSaveBusy,
  runtime,
  runtimeStatus,
  runtimeReady,
  runtimeInstalling,
  finalModel,
  finalModelId,
  finalModelStatus,
  finalModelReady,
  finalModelInstalling,
  liveModel,
  liveModelId,
  liveModelStatus,
  liveModelReady,
  liveModelInstalling,
  deletingModelId,
  onInstallRuntime,
  onInstallModel,
  onDeleteModel,
}: VoiceSecretaryLocalAsrControlsProps) {
  const { t } = useTranslation(["settings", "common"]);

  return (
    <div className={`mt-4 space-y-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
      <div className="flex flex-wrap items-start justify-between gap-3 rounded-lg border border-cyan-500/20 bg-cyan-500/5 p-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <div className="text-xs font-semibold text-[var(--color-text-primary)]">
              {t("assistants.streamingRuntimeTitle")}
            </div>
            <StatusPill tone={runtimeReady ? "on" : runtimeStatus === "failed" ? "off" : "info"}>
              {t("assistants.serviceRuntimeStatusShort", { status: runtimeStatus })}
            </StatusPill>
          </div>
          <p className="mt-1 text-[11px] leading-5 text-[var(--color-text-muted)]">
            {t("assistants.streamingRuntimeHint")}
          </p>
          {runtime?.error?.message ? (
            <p className="mt-1 text-[11px] leading-5 text-rose-700 dark:text-rose-300">
              {t("assistants.serviceRuntimeError", { message: String(runtime.error.message || "") })}
            </p>
          ) : null}
        </div>
        <button
          type="button"
          onClick={onInstallRuntime}
          disabled={busy || voiceSaveBusy || runtimeInstalling || runtimeReady}
          className={secondaryButtonClass("sm")}
        >
          {runtimeReady
            ? t("assistants.streamingRuntimeInstalled")
            : runtimeInstalling
              ? t("assistants.streamingRuntimeInstalling")
              : t("assistants.streamingRuntimeInstall")}
        </button>
      </div>
      <AsrModelCard
        role="final"
        model={finalModel}
        modelId={finalModelId}
        status={finalModelStatus}
        ready={finalModelReady}
        installing={finalModelInstalling}
        busy={busy}
        voiceSaveBusy={voiceSaveBusy}
        deleting={deletingModelId === finalModelId}
        onInstall={onInstallModel}
        onDelete={onDeleteModel}
      />
      <AsrModelCard
        role="live"
        model={liveModel}
        modelId={liveModelId}
        status={liveModelStatus}
        ready={liveModelReady}
        installing={liveModelInstalling}
        busy={busy}
        voiceSaveBusy={voiceSaveBusy}
        deleting={deletingModelId === liveModelId}
        onInstall={onInstallModel}
        onDelete={onDeleteModel}
      />
    </div>
  );
}
