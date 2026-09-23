import type { AssistantVoiceDocument } from "../../../types";

export function documentNeedsContentLoad(
  document: AssistantVoiceDocument | null | undefined,
): boolean {
  if (!document) return false;
  if (document.content !== undefined) return false;
  return Number(document.content_chars || 0) > 0;
}

export function documentContentLoadingMatches(loadingPath: string, documentPath: string): boolean {
  const loading = String(loadingPath || "").trim();
  const active = String(documentPath || "").trim();
  return Boolean(loading && active && loading === active);
}

/**
 * Tracks which in-flight content load owns the visible loading indicator.
 * Loads can overlap (a poll and a stop-triggered refresh, or a refresh and a
 * manual selection); only the most recent one may clear the indicator, and it
 * must do so even when its surrounding refresh was superseded, otherwise the
 * indicator is orphaned.
 */
export type DocumentContentLoadToken = number;

export function createDocumentContentLoadTracker() {
  let latest: DocumentContentLoadToken = 0;
  return {
    begin(): DocumentContentLoadToken {
      latest += 1;
      return latest;
    },
    /** True when `token` is still the newest load, i.e. the caller must clear the indicator. */
    end(token: DocumentContentLoadToken): boolean {
      return token === latest;
    },
    reset(): void {
      latest += 1;
    },
  };
}
