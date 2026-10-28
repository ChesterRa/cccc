import type { Summary } from "./usePendingSummary";

export type PendingIndicator = {
  text: string;
  state: "fresh" | "stale" | "unknown";
  count: number | null;
};

function countText(count: number): string {
  return count > 99 ? "99+" : String(count);
}

export function pendingIndicator(summary: Summary | undefined): PendingIndicator | null {
  if (!summary || summary.state === "unknown" || summary.pending === null)
    return { text: "?", state: "unknown", count: null };
  if (summary.state === "stale")
    return {
      text: summary.pending > 0 ? `~${countText(summary.pending)}` : "?",
      state: "stale",
      count: summary.pending,
    };
  return summary.pending > 0
    ? { text: countText(summary.pending), state: "fresh", count: summary.pending }
    : null;
}

/** Keep the active filter visible after the last answer until the user leaves it. */
export function pendingFilterIndicator(
  summary: Summary | undefined,
  active: boolean,
): PendingIndicator | null {
  return pendingIndicator(summary) ?? (active ? { text: "0", state: "fresh", count: 0 } : null);
}

export function aggregatePendingIndicator(
  summaries: Record<string, Summary>,
  groupIds: readonly string[],
): PendingIndicator | null {
  if (!groupIds.length) return null;
  let known = 0;
  let unknown = false;
  for (const id of new Set(groupIds)) {
    const summary = summaries[id];
    if (!summary || summary.state !== "fresh" || summary.pending === null) unknown = true;
    else known = Math.min(Number.MAX_SAFE_INTEGER, known + summary.pending);
  }
  if (unknown)
    return { text: known ? `${countText(known)}+?` : "?", state: "unknown", count: null };
  return known ? { text: countText(known), state: "fresh", count: known } : null;
}
