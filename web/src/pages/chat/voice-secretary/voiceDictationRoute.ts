import type { VoiceSecretaryCaptureMode } from "./voiceSecretaryTypes";

export type VoiceCaptureDispatchTarget = "composer" | "document" | "instruction" | "prompt";

export function voiceCaptureDispatchTarget(params: {
  captureMode: VoiceSecretaryCaptureMode;
  promptAutoRefine?: boolean;
}): VoiceCaptureDispatchTarget {
  if (params.captureMode === "prompt" && !params.promptAutoRefine) return "composer";
  return params.captureMode;
}

export function voiceCaptureTransportMode(
  target: VoiceCaptureDispatchTarget,
): VoiceSecretaryCaptureMode {
  return target === "composer" ? "prompt" : target;
}
