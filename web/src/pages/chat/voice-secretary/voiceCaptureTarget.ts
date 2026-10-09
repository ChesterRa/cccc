import type { VoiceSecretaryCaptureMode } from "./voiceSecretaryTypes";
import { voiceCaptureDispatchTarget, type VoiceCaptureDispatchTarget } from "./voiceDictationRoute";

export type VoicePanelView = "document" | "ask";

export type VoiceCaptureTarget = Readonly<{
  mode: VoiceSecretaryCaptureMode;
  documentPath: string;
  dispatchTarget: VoiceCaptureDispatchTarget;
}>;

/**
 * The composer mode drives the composer microphone; an open workspace records
 * into its visible view. A recording keeps the target chosen when it started.
 */
export function resolveVoiceCaptureTarget(input: {
  recordingTarget: VoiceCaptureTarget | null;
  panelOpen: boolean;
  panelView: VoicePanelView;
  composerMode: VoiceSecretaryCaptureMode;
  promptAutoRefine: boolean;
  viewedDocumentPath: string;
  defaultDocumentPath: string;
}): VoiceCaptureTarget {
  if (input.recordingTarget) return input.recordingTarget;
  const defaultDocumentPath = input.defaultDocumentPath.trim();
  if (!input.panelOpen)
    return {
      mode: input.composerMode,
      documentPath: defaultDocumentPath,
      dispatchTarget: voiceCaptureDispatchTarget({
        captureMode: input.composerMode,
        promptAutoRefine: input.promptAutoRefine,
      }),
    };
  if (input.panelView === "ask")
    return {
      mode: "instruction",
      documentPath: defaultDocumentPath,
      dispatchTarget: "instruction",
    };
  return {
    mode: "document",
    documentPath: input.viewedDocumentPath.trim() || defaultDocumentPath,
    dispatchTarget: "document",
  };
}

export function voicePanelViewForComposerMode(
  mode: VoiceSecretaryCaptureMode,
  current: VoicePanelView,
): VoicePanelView {
  if (mode === "document") return "document";
  if (mode === "instruction") return "ask";
  return current;
}
