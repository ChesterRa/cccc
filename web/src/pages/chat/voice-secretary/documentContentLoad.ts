import type { AssistantVoiceDocument } from "../../../types";

export function documentNeedsContentLoad(document: AssistantVoiceDocument | null | undefined): boolean {
  if (!document) return false;
  if (document.content !== undefined) return false;
  return Number(document.content_chars || 0) > 0;
}

export function documentContentLoadingMatches(loadingPath: string, documentPath: string): boolean {
  const loading = String(loadingPath || "").trim();
  const active = String(documentPath || "").trim();
  return Boolean(loading && active && loading === active);
}
