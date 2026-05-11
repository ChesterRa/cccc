export function getComposerActionVisibility(isSmallScreen: boolean): {
  showPetShortcut: boolean;
  showMessageModeSelector: boolean;
} {
  return {
    showPetShortcut: !isSmallScreen,
    showMessageModeSelector: !isSmallScreen,
  };
}
