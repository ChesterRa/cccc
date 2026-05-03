import {
  DEFAULT_LIVE_SERVICE_MODEL_ID,
  DEFAULT_SERVICE_MODEL_ID,
} from "../../../pages/chat/voice-secretary/voiceServiceModelRuntime";
import type { AssistantServiceModel } from "../../../types";

export { DEFAULT_LIVE_SERVICE_MODEL_ID };

function recordFromUnknown(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

function hasModelConfig(value: unknown): boolean {
  return Object.keys(recordFromUnknown(value)).length > 0;
}

export function supportsOfflineAsrModel(model: AssistantServiceModel | null | undefined): boolean {
  return hasModelConfig(model?.offline);
}

export function supportsStreamingAsrModel(model: AssistantServiceModel | null | undefined): boolean {
  return hasModelConfig(model?.streaming);
}

function withDefaultStreamingConfig(model: AssistantServiceModel | null | undefined): AssistantServiceModel {
  return {
    ...(model || {}),
    model_id: String(model?.model_id || DEFAULT_LIVE_SERVICE_MODEL_ID),
    kind: String(model?.kind || "asr"),
    runtime_id: String(model?.runtime_id || "sherpa_onnx_streaming"),
    title: String(model?.title || "sherpa-onnx streaming Paraformer trilingual zh/cantonese/en int8"),
    status: String(model?.status || "not_installed"),
    streaming: supportsStreamingAsrModel(model) ? model?.streaming : { engine: "paraformer" },
  };
}

export function resolveLocalAsrModels(args: {
  configuredModelId: string;
  serviceModels: AssistantServiceModel[];
  serviceModelsById: Record<string, AssistantServiceModel>;
}): { finalModel: AssistantServiceModel | null; liveModel: AssistantServiceModel | null } {
  const configuredModel = args.configuredModelId ? args.serviceModelsById[args.configuredModelId] : undefined;
  const defaultFinalModel = args.serviceModelsById[DEFAULT_SERVICE_MODEL_ID];
  const defaultLiveModel = args.serviceModelsById[DEFAULT_LIVE_SERVICE_MODEL_ID];
  const serviceAsrModels = args.serviceModels.filter((model) => String(model.kind || "").trim() === "asr");
  const offlineServiceAsrModels = serviceAsrModels.filter(supportsOfflineAsrModel);
  const streamingServiceAsrModels = serviceAsrModels.filter(supportsStreamingAsrModel);

  const finalModel = (supportsOfflineAsrModel(configuredModel) ? configuredModel : null)
    || (supportsOfflineAsrModel(defaultFinalModel) ? defaultFinalModel : null)
    || offlineServiceAsrModels[0]
    || null;
  const liveModel = (defaultLiveModel ? withDefaultStreamingConfig(defaultLiveModel) : null)
    || streamingServiceAsrModels[0]
    || (supportsStreamingAsrModel(configuredModel) ? configuredModel : null)
    || withDefaultStreamingConfig(null);

  return {
    finalModel,
    liveModel,
  };
}
