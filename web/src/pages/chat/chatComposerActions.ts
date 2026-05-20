export function getComposerActionVisibility(isSmallScreen: boolean): {
  showPetShortcut: boolean;
  showMessageModeSelector: boolean;
} {
  return {
    showPetShortcut: !isSmallScreen,
    showMessageModeSelector: !isSmallScreen,
  };
}

export function getComposerCanSend({
  composerText,
  composerFilesCount,
}: {
  composerText: string;
  composerFilesCount: number;
}): boolean {
  return String(composerText || "").trim().length > 0 || composerFilesCount > 0;
}
