import type { DeviceSessionSource, LocalSessionSource } from "../../types";

export function isDeviceSessionSource(source: LocalSessionSource): source is DeviceSessionSource {
  return source.source_kind === "device";
}

/** Remote sessions join as members; replacing an existing member stays local. */
export function nativeSessionOptions(sources: LocalSessionSource[], currentSourceId?: string) {
  return {
    sources: sources.filter((source) => !isDeviceSessionSource(source)),
    // Keep the current source lookup independent of replacement eligibility.
    currentDirectory: sources.find((source) => source.source_id === currentSourceId)?.cwd || "",
  };
}
