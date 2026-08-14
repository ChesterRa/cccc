import type { VoiceCaptureDispatchTarget } from "./voiceDictationRoute";
import type { VoiceSecretaryCaptureMode } from "./voiceSecretaryTypes";

export type VoiceRecordingSessionScope = Readonly<{
  runId: number;
  sessionId: string;
  groupId: string;
  documentPath: string;
  captureMode: VoiceSecretaryCaptureMode;
  dispatchTarget: VoiceCaptureDispatchTarget;
  composerText: string;
  composerContext: Readonly<Record<string, unknown>>;
}>;

export function createVoiceRecordingSessionScope(input: VoiceRecordingSessionScope) {
  return Object.freeze({
    ...input,
    sessionId: String(input.sessionId || "").trim(),
    groupId: String(input.groupId || "").trim(),
    documentPath: String(input.documentPath || "").trim(),
    composerText: String(input.composerText || ""),
    composerContext: Object.freeze({ ...input.composerContext }),
  }) satisfies VoiceRecordingSessionScope;
}

export function voiceRecordingTargetGroupId(
  scope: VoiceRecordingSessionScope | null,
  selectedGroupId: string,
): string {
  return String(scope?.groupId || selectedGroupId || "").trim();
}

export function voiceRecordingTargetDocumentPath(
  scope: VoiceRecordingSessionScope | null,
  fallbackPath: string,
): string {
  return String(scope?.documentPath || fallbackPath || "").trim();
}

export function voiceRecordingDispatchTarget(
  scope: VoiceRecordingSessionScope | null,
  fallback: VoiceCaptureDispatchTarget,
): VoiceCaptureDispatchTarget {
  return scope?.dispatchTarget || fallback;
}

export function voiceRecordingCaptureMode(
  scope: VoiceRecordingSessionScope | null,
  fallback: VoiceSecretaryCaptureMode,
): VoiceSecretaryCaptureMode {
  return scope?.captureMode || fallback;
}
