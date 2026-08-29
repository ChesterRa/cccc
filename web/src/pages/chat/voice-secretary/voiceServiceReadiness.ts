import type { BuiltinAssistant } from "../../../types";

type VoiceServiceReadinessInput = { assistant?: BuiltinAssistant | null };

function recordFromUnknown(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

export function resolveVoiceServiceReadiness(input: VoiceServiceReadinessInput) {
  const assistant = input.assistant || null;
  const recognitionBackend = String(assistant?.config?.recognition_backend || "browser_asr").trim();
  const serviceHealth = recordFromUnknown(recordFromUnknown(assistant?.health).service);
  const serviceStreamingBackend = recordFromUnknown(serviceHealth.streaming_backend);
  const serviceAsrConfigured = Boolean(serviceHealth.ready || serviceStreamingBackend.ready);
  return {
    assistantEnabled: Boolean(assistant?.enabled),
    recognitionBackend,
    serviceAsrReady: recognitionBackend === "assistant_service_local_asr",
    serviceAsrConfigured,
  };
}
