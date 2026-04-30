import type { AssistantServiceRuntime, BuiltinAssistant } from "../../../types";

type VoiceServiceReadinessInput = {
  assistant?: BuiltinAssistant | null;
  serviceRuntimesById?: Record<string, AssistantServiceRuntime>;
  streamingRuntimeId: string;
};

function recordFromUnknown(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

export function resolveVoiceServiceReadiness(input: VoiceServiceReadinessInput) {
  const assistant = input.assistant || null;
  const recognitionBackend = String(assistant?.config?.recognition_backend || "browser_asr").trim();
  const serviceHealth = recordFromUnknown(recordFromUnknown(assistant?.health).service);
  const serviceManagedModel = recordFromUnknown(serviceHealth.managed_model);
  const serviceStreamingBackend = recordFromUnknown(serviceHealth.streaming_backend);
  const streamingRuntimeReady = String(input.serviceRuntimesById?.[input.streamingRuntimeId]?.status || "").trim() === "ready";
  const serviceAsrConfigured = Boolean(
    serviceStreamingBackend.ready
    || serviceHealth.asr_command_configured
    || serviceHealth.asr_mock_configured
    || serviceHealth.managed_asr_command_configured
    || serviceManagedModel.command_ready,
  );
  return {
    assistantEnabled: Boolean(assistant?.enabled),
    recognitionBackend,
    serviceAsrReady: recognitionBackend === "assistant_service_local_asr",
    streamingRuntimeReady,
    serviceAsrConfigured,
  };
}
