import { useContext } from "react";
import { PendingSummaryContext } from "../pendingReplies/context";
import { aggregatePendingIndicator, type PendingIndicator } from "../pendingReplies/indicator";
import type { Summary } from "../pendingReplies/usePendingSummary";

const known = (summary: Summary) => summary.state === "fresh" && summary.pending !== null;

/**
 * Groups whose requests the inbox reads: every Group with requests waiting, the
 * longest-waiting first, then every Group whose count could not be read, since a count
 * that failed is never taken for zero.
 */
export function waitingGroups(summaries: Record<string, Summary>): string[] {
  const entries = Object.entries(summaries);
  const counted = entries
    .filter(([, summary]) => known(summary) && (summary.pending ?? 0) > 0)
    .sort(([, a], [, b]) => (a.oldestTs ?? "").localeCompare(b.oldestTs ?? ""));
  const uncertain = entries.filter(([, summary]) => !known(summary));
  return [...counted, ...uncertain].map(([groupId]) => groupId);
}

/** What is waiting across all Groups: null only when every Group is known to have none. */
export function waitingIndicator(summaries: Record<string, Summary>): PendingIndicator | null {
  return aggregatePendingIndicator(summaries, Object.keys(summaries));
}

/** What is waiting for the user now, for the places that lead to the list. */
export function useChetWaitingIndicator(): PendingIndicator | null {
  const summary = useContext(PendingSummaryContext);
  return summary ? waitingIndicator(summary.summaries) : null;
}
