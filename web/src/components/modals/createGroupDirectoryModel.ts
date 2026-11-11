import type { DirSuggestion } from "../../types";

export function directoryNameFromPath(path: string): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  return (
    normalized
      .split(/[\\/]+/)
      .filter(Boolean)
      .pop() || ""
  );
}

export function driveSuggestions(suggestions: DirSuggestion[]): DirSuggestion[] {
  return suggestions.filter((suggestion) => suggestion.icon === "drive");
}

/** The server names the home folder and its own working folder in English; every other
 *  suggestion is a real folder or drive name and is shown as it is. */
export function suggestionLabelKey(suggestion: DirSuggestion): string | undefined {
  switch (suggestion.icon) {
    case "home":
      return "modals:createGroup.suggestionHome";
    case "current":
      return "modals:createGroup.suggestionCurrent";
    default:
      return undefined;
  }
}

export function directoryCreationErrorKey(code?: string): string | undefined {
  switch (code) {
    case "ALREADY_EXISTS":
      return "modals:createGroup.folderAlreadyExists";
    case "INVALID_NAME":
      return "modals:createGroup.folderInvalidName";
    case "PERMISSION":
      return "modals:createGroup.folderPermissionDenied";
    default:
      return undefined;
  }
}
