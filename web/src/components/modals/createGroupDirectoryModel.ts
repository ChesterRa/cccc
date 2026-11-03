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
