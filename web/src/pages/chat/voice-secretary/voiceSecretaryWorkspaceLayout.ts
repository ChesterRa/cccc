import type { VoiceSecretaryCaptureMode } from "../VoiceSecretaryComposerControl";

export function getVoiceSecretaryWorkspaceVisibility(args: {
  captureMode: VoiceSecretaryCaptureMode;
  isSmallScreen: boolean;
}): {
  showDocumentList: boolean;
  showWorkspace: boolean;
  showRequestPanel: boolean;
  showRequestCard: boolean;
  showActivityFeed: boolean;
} {
  if (!args.isSmallScreen) {
    return {
      showDocumentList: true,
      showWorkspace: true,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    };
  }
  if (args.captureMode === "document") {
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
