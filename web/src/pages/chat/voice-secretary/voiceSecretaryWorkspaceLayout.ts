import type { VoiceSecretaryCaptureMode } from "../VoiceSecretaryComposerControl";

export function getVoiceSecretaryWorkspaceVisibility(args: {
  captureMode: VoiceSecretaryCaptureMode;
  isSmallScreen: boolean;
  documentRevealed?: boolean;
}): {
  showDocumentList: boolean;
  showWorkspace: boolean;
  showRequestPanel: boolean;
  showRequestCard: boolean;
  showActivityFeed: boolean;
} {
  const showDocument = args.captureMode === "document" || args.documentRevealed;
  if (!args.isSmallScreen && showDocument) {
    return {
      showDocumentList: true,
      showWorkspace: true,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    };
  }
  if (showDocument) {
    return {
      showDocumentList: false,
      showWorkspace: true,
      showRequestPanel: false,
      showRequestCard: false,
      showActivityFeed: false,
    };
  }
  return {
    showDocumentList: false,
    showWorkspace: false,
    showRequestPanel: true,
    showRequestCard: true,
    showActivityFeed: true,
  };
}
