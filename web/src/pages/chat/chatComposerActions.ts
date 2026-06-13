export function getComposerActionVisibility(isSmallScreen: boolean): {
  showMessageModeSelector: boolean;
} {
  return {
    showMessageModeSelector: !isSmallScreen,
  };
}

export function getComposerCanSend({
  composerText,
  composerFilesCount,
  recipientResolutionBusy = false,
}: {
  composerText: string;
  composerFilesCount: number;
  recipientResolutionBusy?: boolean;
}): boolean {
  if (recipientResolutionBusy) return false;
  return String(composerText || "").trim().length > 0 || composerFilesCount > 0;
}
