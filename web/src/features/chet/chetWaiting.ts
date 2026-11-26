import { useContext } from "react";
import { PendingSummaryContext } from "../pendingReplies/context";
import type { Summary } from "../pendingReplies/usePendingSummary";

/** Groups with requests waiting for the user, the longest-waiting first. */
export function waitingGroups(summaries: Record<string, Summary>): string[] {
  return Object.entries(summaries)
    .filter(([, summary]) => (summary.pending ?? 0) > 0)
    .sort(([, a], [, b]) => (a.oldestTs ?? "").localeCompare(b.oldestTs ?? ""))
    .map(([groupId]) => groupId);
}

/** Every request waiting for the user, across all Groups. */
export function waitingTotal(summaries: Record<string, Summary>): number {
  return Object.values(summaries).reduce((total, summary) => total + (summary.pending ?? 0), 0);
}

/** What is waiting for the user now, for the places that lead to the list. */
export function useChetWaitingTotal(): number {
  const summary = useContext(PendingSummaryContext);
  return summary ? waitingTotal(summary.summaries) : 0;
}
