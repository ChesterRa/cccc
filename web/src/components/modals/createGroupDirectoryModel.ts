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
