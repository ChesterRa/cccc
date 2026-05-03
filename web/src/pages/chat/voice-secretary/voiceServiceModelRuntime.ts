import type { AssistantServiceModel, BuiltinAssistant } from "../../../types";

export const STREAMING_ASR_RUNTIME_ID = "sherpa_onnx_streaming";
export const DEFAULT_LIVE_SERVICE_MODEL_ID = "sherpa_onnx_streaming_paraformer_trilingual_zh_cantonese_en";
export const DEFAULT_SERVICE_MODEL_ID = "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8";

const LEGACY_DEFAULT_SERVICE_MODEL_IDS = new Set([""]);

export function effectiveServiceModelId(value: unknown): string {
  const raw = String(value || "").trim();
  return LEGACY_DEFAULT_SERVICE_MODEL_IDS.has(raw) ? DEFAULT_SERVICE_MODEL_ID : raw;
}

export function resolveServiceModelRuntimeId(
  assistant: BuiltinAssistant | null | undefined,
  serviceModelsById: Record<string, AssistantServiceModel>,
): string {
  const modelId = effectiveServiceModelId(assistant?.config?.service_model_id);
  const liveModel = serviceModelsById[DEFAULT_LIVE_SERVICE_MODEL_ID];
  const modelRuntimeId = String(liveModel?.runtime_id || serviceModelsById[modelId]?.runtime_id || "").trim();
  if (modelRuntimeId) return modelRuntimeId;
  return STREAMING_ASR_RUNTIME_ID;
}
