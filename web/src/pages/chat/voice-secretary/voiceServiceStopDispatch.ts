import type { VoiceSecretaryCaptureMode } from "./voiceSecretaryTypes";

export type VoiceServiceStopDispatchKind = "" | "prompt" | "instruction";

export function voiceServiceStopDispatchKind(params: {
  mode: VoiceSecretaryCaptureMode;
  transcriptText: string;
  pendingPromptRequestId?: string;
  pendingAskRequestId?: string;
}): VoiceServiceStopDispatchKind {
  const text = String(params.transcriptText || "").trim();
  if (!text) return "";
  if (params.mode === "prompt" && !String(params.pendingPromptRequestId || "").trim()) return "prompt";
  if (params.mode === "instruction" && !String(params.pendingAskRequestId || "").trim()) return "instruction";
  return "";
}
