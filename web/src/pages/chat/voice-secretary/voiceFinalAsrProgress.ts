import type { TFunction } from "i18next";

type FinalAsrPayload = Record<string, unknown>;

function numberField(payload: FinalAsrPayload, key: string): number {
  const value = Number(payload[key]);
  return Number.isFinite(value) ? value : 0;
}

export function voiceFinalAsrProgressLabel(payload: FinalAsrPayload, t: TFunction): string {
  const type = String(payload.type || "").trim();
  if (type === "final_asr_started") {
    return t("voiceSecretaryFinalAsrStarted", { defaultValue: "Preparing final transcript..." });
  }
  if (type === "final_asr_failed") {
    return t("voiceSecretaryFinalAsrFallback", { defaultValue: "Final ASR failed; keeping live transcript." });
  }
  const stage = String(payload.stage || "").trim();
  if (stage === "vad_fallback") {
    return t("voiceSecretaryFinalAsrVadFallback", { defaultValue: "Detecting speech with fallback segmentation..." });
  }
  if (stage === "segments_ready") {
    const count = numberField(payload, "segment_count");
    return count > 0
      ? t("voiceSecretaryFinalAsrSegmentsReady", { count, defaultValue: "Prepared {{count}} speech segments." })
      : t("voiceSecretaryFinalAsrNoSegments", { defaultValue: "No speech segments detected." });
  }
  if (stage === "model_loading") {
    return t("voiceSecretaryFinalAsrLoadingModel", { defaultValue: "Loading final ASR model..." });
  }
  if (stage === "legacy_fallback") {
    return t("voiceSecretaryFinalAsrLegacyFallback", { defaultValue: "Using fallback final transcription path..." });
  }
  if (stage === "transcribing") {
    const index = numberField(payload, "segment_index");
    const count = numberField(payload, "segment_count");
    return index > 0 && count > 0
      ? t("voiceSecretaryFinalAsrTranscribingSegment", {
          index,
          count,
          defaultValue: "Final ASR {{index}}/{{count}}...",
        })
      : t("voiceSecretaryFinalAsrTranscribing", { defaultValue: "Running final ASR..." });
  }
  return t("voiceSecretaryFinalizingRecording", { defaultValue: "Finalizing" });
}
