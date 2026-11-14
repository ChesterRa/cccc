import type { LocalSessionSource } from "../../types";

/** Remote sessions join as members; replacing an existing member stays local. */
export function nativeSessionOptions(sources: LocalSessionSource[], currentSourceId?: string) {
  return {
    sources: sources.filter((source) => !source.source_id.startsWith("remote:")),
    // Keep the current source lookup independent of replacement eligibility.
    currentDirectory: sources.find((source) => source.source_id === currentSourceId)?.cwd || "",
  };
}
