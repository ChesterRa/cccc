import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import type { AssistantServiceModel, AssistantStateResult } from "../../../types";
import { DEFAULT_SERVICE_MODEL_ID } from "../../../pages/chat/voice-secretary/voiceServiceModelRuntime";
import { resolveLocalAsrModels } from "./assistantsLocalAsrModels";
import { primaryButtonClass, secondaryButtonClass } from "./types";
const DIARIZATION_MODEL_ID = "sherpa_onnx_diarization_pyannote_3dspeaker_zh";
function formatModelSize(bytes: number | undefined): string {
  const value = Number(bytes || 0);
  if (!Number.isFinite(value) || value <= 0) return "";
  if (value >= 1024 * 1024 * 1024) return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
  if (value >= 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KiB`;
  return `${Math.round(value)} B`;
}

function firstArtifact(model: AssistantServiceModel | null | undefined) {
  return model?.artifacts && model.artifacts.length > 0 ? model.artifacts[0] : null;
}

function shortHash(value: string | undefined): string {
  const raw = String(value || "").trim();
  return raw.length > 16 ? `${raw.slice(0, 12)}...${raw.slice(-6)}` : raw;
}

function serviceModelStatusLabel(
  status: string,
  model: AssistantServiceModel | null | undefined,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (
    status === "downloading" &&
    Number(model?.progress_percent || 0) >= 100 &&
    model?.installed !== true
  ) {
    return t("assistants.componentStatusShort", {
      status: "installing",
      defaultValue: "{{status}}",
    });
  }
  if (status !== "downloading")
    return t("assistants.componentStatusShort", { status, defaultValue: "{{status}}" });
  return `${t("assistants.componentStatusShort", { status, defaultValue: "{{status}}" })} ${Math.round(Number(model?.progress_percent || 0))}%`;
}

function StatusPill({ children, tone }: { children: ReactNode; tone: "on" | "off" | "info" }) {
  const classes =
    tone === "on"
      ? "border border-emerald-600/15 bg-emerald-50 text-emerald-800 dark:border-emerald-400/18 dark:bg-emerald-400/10 dark:text-emerald-200"
      : tone === "off"
        ? "border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-muted)]"
        : "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white";
  return (
    <span
      className={`inline-flex items-center rounded-full px-2.5 py-1 text-xs font-medium shadow-[inset_0_1px_0_rgba(255,255,255,0.55)] ${classes}`}
    >
      {children}
    </span>
  );
}

function localVoicePanelClass() {
  return "rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-4";
}

function localVoiceModelCardClass() {
  return "rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] p-3";
}

export function LocalAsrModels({
  isActive,
  busy = false,
  configuredModelId = DEFAULT_SERVICE_MODEL_ID,
  onBusyChange,
}: {
  isActive: boolean;
  busy?: boolean;
  configuredModelId?: string;
  onBusyChange?: (busy: boolean) => void;
}) {
  const { t } = useTranslation("settings");
  const [assistantState, setAssistantState] = useState<AssistantStateResult | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [localAsrSetupBusy, setLocalAsrSetupBusy] = useState(false);
  const [diarizationModelInstallBusy, setDiarizationModelInstallBusy] = useState(false);
  const [localAsrMaintenanceBusy, setLocalAsrMaintenanceBusy] = useState(false);
  const mounted = useRef(true);
  const sequence = useRef(0);
  const invalidateLoads = useCallback(() => {
    sequence.current++;
  }, []);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      invalidateLoads();
    };
  }, [invalidateLoads]);
  const loadAssistants = useCallback(
    async (_options?: { quiet?: boolean }) => {
      const seq = ++sequence.current;
      try {
        const response = await api.fetchGlobalAsrModels();
        if (!mounted.current || seq !== sequence.current) return;
        if (response.ok) setAssistantState(response.result);
        else setError(response.error.message);
      } catch {
        if (mounted.current && seq === sequence.current) setError(t("assistants.loadFailed"));
      }
    },
    [t],
  );
  useEffect(() => {
    if (isActive) void loadAssistants();
  }, [isActive, loadAssistants]);
  const serviceModelsById = assistantState?.service_models_by_id || {};
  const { finalModel: finalServiceAsrModel, liveModel: liveServiceAsrModel } =
    resolveLocalAsrModels({
      configuredModelId,
      serviceModels: assistantState?.service_models || [],
      serviceModelsById,
    });
  const streamingRuntime = assistantState?.service_runtime;
  const streamingRuntimeStatus =
    String(streamingRuntime?.status || "not_installed").trim() || "not_installed";
  const streamingRuntimeReady = streamingRuntimeStatus === "ready";
  const streamingRuntimeInstalledVersion = String(streamingRuntime?.installed_version || "").trim();
  const finalServiceAsrModelId = String(finalServiceAsrModel?.model_id || "").trim();
  const finalServiceAsrModelStatus =
    String(finalServiceAsrModel?.status || "not_installed").trim() || "not_installed";
  const finalServiceAsrModelInstalling =
    finalServiceAsrModelStatus === "downloading" || finalServiceAsrModelStatus === "installing";
  const finalServiceAsrModelReady = finalServiceAsrModelStatus === "ready";
  const finalServiceAsrModelUpdateAvailable = Boolean(finalServiceAsrModel?.update_available);
  const finalServiceAsrModelSize = formatModelSize(finalServiceAsrModel?.total_size_bytes);
  const finalServiceAsrArtifact = firstArtifact(finalServiceAsrModel);
  const liveServiceAsrModelId = String(liveServiceAsrModel?.model_id || "").trim();
  const liveServiceAsrModelStatus =
    String(liveServiceAsrModel?.status || "not_installed").trim() || "not_installed";
  const liveServiceAsrModelInstalling =
    liveServiceAsrModelStatus === "downloading" || liveServiceAsrModelStatus === "installing";
  const liveServiceAsrModelReady = liveServiceAsrModelStatus === "ready";
  const liveServiceAsrModelUpdateAvailable = Boolean(liveServiceAsrModel?.update_available);
  const liveServiceAsrModelSize = formatModelSize(liveServiceAsrModel?.total_size_bytes);
  const liveServiceAsrArtifact = firstArtifact(liveServiceAsrModel);
  const diarizationModel = serviceModelsById[DIARIZATION_MODEL_ID];
  const diarizationModelStatus =
    String(diarizationModel?.status || "not_installed").trim() || "not_installed";
  const diarizationModelInstalling =
    diarizationModelStatus === "downloading" || diarizationModelStatus === "installing";
  const diarizationModelReady = diarizationModelStatus === "ready";
  const diarizationModelUpdateAvailable = Boolean(diarizationModel?.update_available);
  const diarizationModelSize = formatModelSize(diarizationModel?.total_size_bytes);
  const diarizationModelDiskSize = formatModelSize(diarizationModel?.disk_usage_bytes);
  const diarizationModelArtifact = firstArtifact(diarizationModel);
  const localAsrInstalling =
    localAsrSetupBusy || liveServiceAsrModelInstalling || finalServiceAsrModelInstalling;
  const localAsrReady =
    streamingRuntimeReady && liveServiceAsrModelReady && finalServiceAsrModelReady;
  const localAsrFailed =
    streamingRuntimeStatus === "failed" ||
    liveServiceAsrModelStatus === "failed" ||
    finalServiceAsrModelStatus === "failed";
  const localAsrUpdateAvailable =
    liveServiceAsrModelUpdateAvailable || finalServiceAsrModelUpdateAvailable;
  const localAsrStatusTone: "on" | "off" | "info" =
    localAsrReady && !localAsrUpdateAvailable ? "on" : localAsrFailed ? "off" : "info";
  const localAsrStatusLabel = localAsrInstalling
    ? t("assistants.localAsrInstalling", { defaultValue: "Installing" })
    : localAsrUpdateAvailable
      ? t("assistants.localAsrUpdateAvailable", { defaultValue: "Update available" })
      : localAsrReady
        ? t("assistants.localAsrReady", { defaultValue: "Ready" })
        : localAsrFailed
          ? t("assistants.localAsrFailed", { defaultValue: "Needs repair" })
          : t("assistants.localAsrSetupNeeded", { defaultValue: "Setup needed" });
  const localAsrDiskUsage = formatModelSize(
    Number(liveServiceAsrModel?.disk_usage_bytes || 0) +
      Number(finalServiceAsrModel?.disk_usage_bytes || 0),
  );
  const selectedServiceModelInstalling =
    localAsrSetupBusy ||
    liveServiceAsrModelInstalling ||
    finalServiceAsrModelInstalling ||
    diarizationModelInstallBusy ||
    diarizationModelInstalling ||
    localAsrMaintenanceBusy;
  const localAsrModelIds = Array.from(
    new Set([liveServiceAsrModelId, finalServiceAsrModelId].filter(Boolean)),
  );
  const canManageLocalAsr = localAsrModelIds.length > 0;

  const installLocalAsrModel = async (modelId: string): Promise<boolean> => {
    if (!modelId) return false;
    const resp = await api.installGlobalAsrModel(modelId);
    if (!mounted.current) return false;
    if (!resp.ok) {
      setError(
        resp.error?.message ||
          t("assistants.streamingAsrModelInstallFailed", {
            defaultValue: "Failed to install ASR model.",
          }),
      );
      return false;
    }
    return true;
  };

  const installLocalAsrBundle = async () => {
    if (!canManageLocalAsr) return;
    setLocalAsrSetupBusy(true);
    setError("");
    setNotice("");
    try {
      const updating = localAsrUpdateAvailable;
      if (
        (!liveServiceAsrModelReady || liveServiceAsrModelUpdateAvailable) &&
        liveServiceAsrModelId
      ) {
        const installed = await installLocalAsrModel(liveServiceAsrModelId);
        if (!installed) return;
      }
      if (
        (!finalServiceAsrModelReady || finalServiceAsrModelUpdateAvailable) &&
        finalServiceAsrModelId
      ) {
        const installed = await installLocalAsrModel(finalServiceAsrModelId);
        if (!installed) return;
      }
      setNotice(
        updating
          ? t("assistants.localAsrUpdateStarted", { defaultValue: "Local ASR update started." })
          : t("assistants.localAsrInstallStarted", { defaultValue: "Local ASR setup started." }),
      );
      await loadAssistants({ quiet: true });
    } catch {
      if (!mounted.current) return;
      setError(
        t("assistants.localAsrInstallFailed", { defaultValue: "Failed to install local ASR." }),
      );
    } finally {
      if (mounted.current) setLocalAsrSetupBusy(false);
    }
  };

  const installDiarizationModel = async () => {
    setDiarizationModelInstallBusy(true);
    setError("");
    setNotice("");
    try {
      const resp = await api.installGlobalAsrModel(DIARIZATION_MODEL_ID);
      if (!mounted.current) return;
      if (!resp.ok) {
        setError(
          resp.error?.message ||
            t("assistants.diarizationModelInstallFailed", {
              defaultValue: "Failed to install speaker diarization model.",
            }),
        );
        return;
      }
      setNotice(
        diarizationModelUpdateAvailable
          ? t("assistants.diarizationModelUpdateStarted", {
              defaultValue: "Speaker-label model update started.",
            })
          : t("assistants.diarizationModelInstallStarted", {
              defaultValue: "Speaker diarization model download started.",
            }),
      );
      await loadAssistants({ quiet: true });
    } catch {
      if (!mounted.current) return;
      setError(
        t("assistants.diarizationModelInstallFailed", {
          defaultValue: "Failed to install speaker diarization model.",
        }),
      );
    } finally {
      if (mounted.current) setDiarizationModelInstallBusy(false);
    }
  };

  const removeLocalAsrBundle = async () => {
    if (!canManageLocalAsr) return;
    if (
      !window.confirm(
        t("assistants.localAsrRemoveConfirm", {
          defaultValue: "Remove the local ASR models from this device?",
        }),
      )
    )
      return;
    setLocalAsrMaintenanceBusy(true);
    setError("");
    setNotice("");
    try {
      for (const modelId of localAsrModelIds) {
        const modelResp = await api.removeGlobalAsrModel(modelId);
        if (!mounted.current) return;
        if (!modelResp.ok) {
          setError(
            modelResp.error?.message ||
              t("assistants.localAsrRemoveFailed", { defaultValue: "Failed to remove local ASR." }),
          );
          return;
        }
      }
      setNotice(t("assistants.localAsrRemoved", { defaultValue: "Local ASR cache removed." }));
      await loadAssistants({ quiet: true });
    } catch {
      if (!mounted.current) return;
      setError(
        t("assistants.localAsrRemoveFailed", { defaultValue: "Failed to remove local ASR." }),
      );
    } finally {
      if (mounted.current) setLocalAsrMaintenanceBusy(false);
    }
  };

  const reinstallLocalAsrBundle = async () => {
    if (!canManageLocalAsr) return;
    if (
      !window.confirm(
        t("assistants.localAsrReinstallConfirm", {
          defaultValue: "Reinstall the local ASR models?",
        }),
      )
    )
      return;
    setLocalAsrMaintenanceBusy(true);
    setError("");
    setNotice("");
    try {
      for (const modelId of localAsrModelIds) {
        const modelRemove = await api.removeGlobalAsrModel(modelId);
        if (!mounted.current) return;
        if (!modelRemove.ok) {
          setError(
            modelRemove.error?.message ||
              t("assistants.localAsrReinstallFailed", {
                defaultValue: "Failed to reinstall local ASR.",
              }),
          );
          return;
        }
      }
      for (const modelId of localAsrModelIds) {
        const modelInstall = await api.installGlobalAsrModel(modelId);
        if (!mounted.current) return;
        if (!modelInstall.ok) {
          setError(
            modelInstall.error?.message ||
              t("assistants.localAsrReinstallFailed", {
                defaultValue: "Failed to reinstall local ASR.",
              }),
          );
          return;
        }
      }
      setNotice(
        t("assistants.localAsrReinstallStarted", { defaultValue: "Local ASR reinstall started." }),
      );
      await loadAssistants({ quiet: true });
    } catch {
      if (!mounted.current) return;
      setError(
        t("assistants.localAsrReinstallFailed", { defaultValue: "Failed to reinstall local ASR." }),
      );
    } finally {
      if (mounted.current) setLocalAsrMaintenanceBusy(false);
    }
  };

  const removeDiarizationModel = async () => {
    if (
      !window.confirm(
        t("assistants.diarizationModelRemoveConfirm", {
          defaultValue: "Remove the speaker-label model from this device?",
        }),
      )
    )
      return;
    setDiarizationModelInstallBusy(true);
    setError("");
    setNotice("");
    try {
      const resp = await api.removeGlobalAsrModel(DIARIZATION_MODEL_ID);
      if (!mounted.current) return;
      if (!resp.ok) {
        setError(
          resp.error?.message ||
            t("assistants.diarizationModelRemoveFailed", {
              defaultValue: "Failed to remove speaker-label model.",
            }),
        );
        return;
      }
      setNotice(
        t("assistants.diarizationModelRemoved", { defaultValue: "Speaker-label model removed." }),
      );
      await loadAssistants({ quiet: true });
    } catch {
      if (!mounted.current) return;
      setError(
        t("assistants.diarizationModelRemoveFailed", {
          defaultValue: "Failed to remove speaker-label model.",
        }),
      );
    } finally {
      if (mounted.current) setDiarizationModelInstallBusy(false);
    }
  };

  const reinstallDiarizationModel = async () => {
    if (
      !window.confirm(
        t("assistants.diarizationModelReinstallConfirm", {
          defaultValue: "Reinstall the speaker-label model?",
        }),
      )
    )
      return;
    setDiarizationModelInstallBusy(true);
    setError("");
    setNotice("");
    try {
      const removeResp = await api.removeGlobalAsrModel(DIARIZATION_MODEL_ID);
      if (!mounted.current) return;
      if (!removeResp.ok) {
        setError(
          removeResp.error?.message ||
            t("assistants.diarizationModelReinstallFailed", {
              defaultValue: "Failed to reinstall speaker-label model.",
            }),
        );
        return;
      }
      const installResp = await api.installGlobalAsrModel(DIARIZATION_MODEL_ID);
      if (!mounted.current) return;
      if (!installResp.ok) {
        setError(
          installResp.error?.message ||
            t("assistants.diarizationModelReinstallFailed", {
              defaultValue: "Failed to reinstall speaker-label model.",
            }),
        );
        return;
      }
      setNotice(
        t("assistants.diarizationModelReinstallStarted", {
          defaultValue: "Speaker-label model reinstall started.",
        }),
      );
      await loadAssistants({ quiet: true });
    } catch {
      if (!mounted.current) return;
      setError(
        t("assistants.diarizationModelReinstallFailed", {
          defaultValue: "Failed to reinstall speaker-label model.",
        }),
      );
    } finally {
      if (mounted.current) setDiarizationModelInstallBusy(false);
    }
  };

  const actionBusy = localAsrSetupBusy || diarizationModelInstallBusy || localAsrMaintenanceBusy;
  useEffect(() => {
    onBusyChange?.(actionBusy);
    return () => onBusyChange?.(false);
  }, [onBusyChange, actionBusy]);
  useEffect(() => {
    if (!isActive || !selectedServiceModelInstalling) return;
    const timer = window.setInterval(() => {
      void loadAssistants({ quiet: true });
    }, 1500);
    return () => window.clearInterval(timer);
  }, [isActive, loadAssistants, selectedServiceModelInstalling]);
  if (!assistantState) {
    return (
      <section aria-label={t("assistants.localAsrTitle")} className="mt-4 space-y-3">
        <p role={error ? "alert" : "status"} className="text-sm text-[var(--color-text-secondary)]">
          {error || t("common:loading")}
        </p>
        {error && (
          <button
            type="button"
            className={secondaryButtonClass("sm")}
            disabled={!isActive || busy}
            onClick={() => {
              setError("");
              void loadAssistants();
            }}
          >
            {t("voiceSettings.refresh")}
          </button>
        )}
      </section>
    );
  }
  const sharedModels = (
    <div className={"mt-4 min-w-0 space-y-4"}>
      <div className={localVoicePanelClass()}>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                {t("assistants.localAsrTitle", { defaultValue: "Local ASR" })}
              </div>
              <StatusPill tone={localAsrStatusTone}>{localAsrStatusLabel}</StatusPill>
              {localAsrDiskUsage ? <StatusPill tone="info">{localAsrDiskUsage}</StatusPill> : null}
            </div>
            <p className="mt-1 max-w-2xl text-xs leading-5 text-[var(--color-text-muted)]">
              {t("assistants.localAsrHint", {
                defaultValue:
                  "Download the local speech models used for private transcription on this device. The sherpa-onnx engine is built into CCCC.",
              })}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void installLocalAsrBundle()}
            disabled={
              busy ||
              selectedServiceModelInstalling ||
              !canManageLocalAsr ||
              (localAsrReady && !localAsrUpdateAvailable)
            }
            className={
              localAsrReady && !localAsrUpdateAvailable
                ? secondaryButtonClass("sm")
                : primaryButtonClass(false)
            }
          >
            {localAsrInstalling
              ? t("assistants.localAsrInstalling", { defaultValue: "Installing" })
              : localAsrUpdateAvailable
                ? t("assistants.localAsrUpdate", { defaultValue: "Update local ASR" })
                : localAsrReady
                  ? t("assistants.localAsrUpToDate", { defaultValue: "Up to date" })
                  : localAsrFailed
                    ? t("assistants.localAsrRepair", { defaultValue: "Repair local ASR" })
                    : t("assistants.localAsrInstall", { defaultValue: "Install local ASR" })}
          </button>
        </div>
        <div className="mt-4 grid gap-2 lg:grid-cols-3">
          <div className={localVoiceModelCardClass()}>
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-xs font-semibold uppercase tracking-[0.14em] text-[var(--color-text-muted)]">
                {t("assistants.localAsrEngineLabel", { defaultValue: "Engine" })}
              </span>
              <StatusPill
                tone={
                  streamingRuntimeReady
                    ? "on"
                    : streamingRuntimeStatus === "failed"
                      ? "off"
                      : "info"
                }
              >
                {t("assistants.componentStatusShort", {
                  status: streamingRuntimeStatus,
                  defaultValue: "{{status}}",
                })}
              </StatusPill>
            </div>
            <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
              {t("assistants.localAsrEngineHint", {
                defaultValue: "Built into the CCCC executable.",
              })}
            </p>
            {streamingRuntimeInstalledVersion ? (
              <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
                {t("assistants.localAsrEngineInstalledVersion", {
                  version: streamingRuntimeInstalledVersion,
                  defaultValue: "Version {{version}}",
                })}
              </p>
            ) : null}
          </div>
          <div className={localVoiceModelCardClass()}>
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-xs font-semibold uppercase tracking-[0.14em] text-[var(--color-text-muted)]">
                {t("assistants.liveAsrModelLabel", { defaultValue: "Live ASR" })}
              </span>
              <StatusPill
                tone={
                  liveServiceAsrModelReady
                    ? "on"
                    : liveServiceAsrModelStatus === "failed"
                      ? "off"
                      : "info"
                }
              >
                {serviceModelStatusLabel(liveServiceAsrModelStatus, liveServiceAsrModel, t)}
              </StatusPill>
              {liveServiceAsrModelUpdateAvailable ? (
                <StatusPill tone="info">
                  {t("assistants.updateAvailable", { defaultValue: "Update available" })}
                </StatusPill>
              ) : null}
              {liveServiceAsrModelSize ? (
                <StatusPill tone="info">{liveServiceAsrModelSize}</StatusPill>
              ) : null}
            </div>
            <p className="mt-1 break-words text-xs leading-5 text-[var(--color-text-muted)]">
              {liveServiceAsrModel?.title ||
                liveServiceAsrModelId ||
                t("assistants.streamingAsrModelMissing", {
                  defaultValue: "No streaming ASR model is available.",
                })}
            </p>
          </div>
          <div className={localVoiceModelCardClass()}>
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-xs font-semibold uppercase tracking-[0.14em] text-[var(--color-text-muted)]">
                {t("assistants.finalAsrModelLabel", { defaultValue: "Final ASR" })}
              </span>
              <StatusPill
                tone={
                  finalServiceAsrModelReady
                    ? "on"
                    : finalServiceAsrModelStatus === "failed"
                      ? "off"
                      : "info"
                }
              >
                {serviceModelStatusLabel(finalServiceAsrModelStatus, finalServiceAsrModel, t)}
              </StatusPill>
              {finalServiceAsrModelUpdateAvailable ? (
                <StatusPill tone="info">
                  {t("assistants.updateAvailable", { defaultValue: "Update available" })}
                </StatusPill>
              ) : null}
              {finalServiceAsrModelSize ? (
                <StatusPill tone="info">{finalServiceAsrModelSize}</StatusPill>
              ) : null}
            </div>
            <p className="mt-1 break-words text-xs leading-5 text-[var(--color-text-muted)]">
              {finalServiceAsrModel?.title ||
                finalServiceAsrModelId ||
                t("assistants.finalAsrModelMissing", {
                  defaultValue: "No final ASR model is available.",
                })}
            </p>
          </div>
        </div>
        {streamingRuntime?.error?.message ||
        liveServiceAsrModel?.error?.message ||
        finalServiceAsrModel?.error?.message ? (
          <p className="mt-3 text-xs leading-5 text-rose-700 dark:text-rose-300">
            {t("assistants.serviceRuntimeError", {
              message: String(
                streamingRuntime?.error?.message ||
                  liveServiceAsrModel?.error?.message ||
                  finalServiceAsrModel?.error?.message ||
                  "",
              ),
            })}
          </p>
        ) : null}
      </div>

      <div className={`flex flex-wrap items-start justify-between gap-3 ${localVoicePanelClass()}`}>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("assistants.speakerLabelsTitle", { defaultValue: "Speaker labels" })}
            </div>
            <StatusPill
              tone={
                diarizationModelReady ? "on" : diarizationModelStatus === "failed" ? "off" : "info"
              }
            >
              {serviceModelStatusLabel(diarizationModelStatus, diarizationModel, t)}
            </StatusPill>
            {diarizationModelUpdateAvailable ? (
              <StatusPill tone="info">
                {t("assistants.updateAvailable", { defaultValue: "Update available" })}
              </StatusPill>
            ) : null}
            <StatusPill tone="info">
              {t("assistants.optional", { defaultValue: "Optional" })}
            </StatusPill>
            {diarizationModelSize ? (
              <StatusPill tone="info">{diarizationModelSize}</StatusPill>
            ) : null}
          </div>
          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
            {t("assistants.speakerLabelsHint", {
              defaultValue:
                "Adds anonymous Speaker 1 / Speaker 2 turns after local ASR recordings. Local transcription works without this model.",
            })}
          </p>
          {diarizationModel?.error?.message ? (
            <p className="mt-1 text-xs leading-5 text-rose-700 dark:text-rose-300">
              {t("assistants.serviceRuntimeError", {
                message: String(diarizationModel.error.message || ""),
              })}
            </p>
          ) : null}
        </div>
        <button
          type="button"
          onClick={() => void installDiarizationModel()}
          disabled={
            busy ||
            diarizationModelInstalling ||
            (diarizationModelReady && !diarizationModelUpdateAvailable)
          }
          className={secondaryButtonClass("sm")}
        >
          {diarizationModelInstalling
            ? t("assistants.diarizationModelInstalling", { defaultValue: "Downloading..." })
            : diarizationModelUpdateAvailable
              ? t("assistants.diarizationModelUpdate", { defaultValue: "Update speaker model" })
              : diarizationModelReady
                ? t("assistants.diarizationModelInstalled", { defaultValue: "Model installed" })
                : t("assistants.diarizationModelInstall", {
                    defaultValue: "Install speaker model",
                  })}
        </button>
      </div>

      <details>
        <summary className="cursor-pointer text-xs font-semibold text-[var(--color-text-secondary)]">
          {t("assistants.localAsrMaintenanceTitle", { defaultValue: "Advanced maintenance" })}
        </summary>
        <div className="mt-3 rounded-xl border border-black/5 bg-white/35 p-3 dark:border-white/10 dark:bg-white/[0.04]">
          <div className="grid gap-2 text-xs leading-5 text-[var(--color-text-muted)] md:grid-cols-2">
            <div>
              <span className="font-semibold text-[var(--color-text-secondary)]">
                {t("assistants.localAsrCacheLabel", { defaultValue: "Local ASR cache" })}:{" "}
              </span>
              {localAsrDiskUsage || t("assistants.none", { defaultValue: "none" })}
            </div>
            <div>
              <span className="font-semibold text-[var(--color-text-secondary)]">
                {t("assistants.speakerLabelsCacheLabel", { defaultValue: "Speaker-label cache" })}
                :{" "}
              </span>
              {diarizationModelDiskSize || t("assistants.none", { defaultValue: "none" })}
            </div>
            <div>
              <span className="font-semibold text-[var(--color-text-secondary)]">
                {t("assistants.localAsrRuntimeVersionLabel", { defaultValue: "Runtime version" })}
                :{" "}
              </span>
              {streamingRuntimeInstalledVersion || "-"}
            </div>
            <div className="break-words md:col-span-2">
              <span className="font-semibold text-[var(--color-text-secondary)]">
                {t("assistants.liveAsrModelPath", { defaultValue: "Live model path" })}:{" "}
              </span>
              {liveServiceAsrModel?.install_dir || "-"}
            </div>
            {liveServiceAsrArtifact?.url ? (
              <div className="break-words md:col-span-2">
                <span className="font-semibold text-[var(--color-text-secondary)]">
                  {t("assistants.liveAsrModelSource", { defaultValue: "Live model source" })}:{" "}
                </span>
                {liveServiceAsrArtifact.url}
                {liveServiceAsrArtifact.sha256
                  ? ` · sha256 ${shortHash(liveServiceAsrArtifact.sha256)}`
                  : ""}
              </div>
            ) : null}
            <div className="break-words md:col-span-2">
              <span className="font-semibold text-[var(--color-text-secondary)]">
                {t("assistants.finalAsrModelPath", { defaultValue: "Final model path" })}:{" "}
              </span>
              {finalServiceAsrModel?.install_dir || "-"}
            </div>
            {finalServiceAsrArtifact?.url ? (
              <div className="break-words md:col-span-2">
                <span className="font-semibold text-[var(--color-text-secondary)]">
                  {t("assistants.finalAsrModelSource", { defaultValue: "Final model source" })}
                  :{" "}
                </span>
                {finalServiceAsrArtifact.url}
                {finalServiceAsrArtifact.sha256
                  ? ` · sha256 ${shortHash(finalServiceAsrArtifact.sha256)}`
                  : ""}
              </div>
            ) : null}
            {diarizationModelArtifact?.url ? (
              <div className="break-words md:col-span-2">
                <span className="font-semibold text-[var(--color-text-secondary)]">
                  {t("assistants.speakerLabelsModelSource", {
                    defaultValue: "Speaker model source",
                  })}
                  :{" "}
                </span>
                {diarizationModelArtifact.url}
                {diarizationModelArtifact.sha256
                  ? ` · sha256 ${shortHash(diarizationModelArtifact.sha256)}`
                  : ""}
              </div>
            ) : null}
          </div>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => void reinstallLocalAsrBundle()}
              disabled={busy || selectedServiceModelInstalling || !canManageLocalAsr}
              className={secondaryButtonClass("sm")}
            >
              {t("assistants.localAsrReinstall", { defaultValue: "Reinstall ASR models" })}
            </button>
            <button
              type="button"
              onClick={() => void removeLocalAsrBundle()}
              disabled={busy || selectedServiceModelInstalling || !canManageLocalAsr}
              className={secondaryButtonClass("sm")}
            >
              {t("assistants.localAsrRemove", { defaultValue: "Remove ASR models" })}
            </button>
            <button
              type="button"
              onClick={() => void reinstallDiarizationModel()}
              disabled={busy || selectedServiceModelInstalling}
              className={secondaryButtonClass("sm")}
            >
              {t("assistants.diarizationModelReinstall", {
                defaultValue: "Reinstall speaker labels",
              })}
            </button>
            <button
              type="button"
              onClick={() => void removeDiarizationModel()}
              disabled={busy || selectedServiceModelInstalling || !diarizationModel?.model_id}
              className={secondaryButtonClass("sm")}
            >
              {t("assistants.diarizationModelRemove", { defaultValue: "Remove speaker labels" })}
            </button>
          </div>
        </div>
      </details>
    </div>
  );

  return (
    <section aria-label={t("assistants.localAsrTitle")}>
      {error && (
        <p role="alert" className="break-words text-sm text-rose-600 dark:text-rose-300">
          {error}
        </p>
      )}
      {!error && notice && (
        <p role="status" className="text-sm text-[var(--color-text-secondary)]">
          {notice}
        </p>
      )}
      {sharedModels}
    </section>
  );
}
