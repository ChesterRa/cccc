export type DocumentFinalAsrDisposition =
  | "preserve_live"
  | "restore_persisted"
  | "retry_persistence";

export function documentFinalAsrDisposition(
  payload: Record<string, unknown>,
): DocumentFinalAsrDisposition {
  if (payload.partial === true) return "preserve_live";
  if (payload.transcript_persistence === "persisted" && payload.transcript_persisted === true) {
    return "restore_persisted";
  }
  return "retry_persistence";
}
