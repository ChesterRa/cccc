import type { VoiceSecretaryCaptureMode } from "./voiceSecretaryTypes";

export type VoiceCaptureDispatchTarget = "composer" | "document" | "instruction" | "prompt";

export function voiceCaptureDispatchTarget(params: {
  assistantEnabled: boolean;
  captureMode: VoiceSecretaryCaptureMode;
}): VoiceCaptureDispatchTarget {
  if (!params.assistantEnabled) return "composer";
  return params.captureMode;
}

export function voiceCaptureTransportMode(
  target: VoiceCaptureDispatchTarget,
): VoiceSecretaryCaptureMode {
  return target === "composer" ? "prompt" : target;
}
