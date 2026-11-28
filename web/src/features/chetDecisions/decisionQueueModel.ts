import type { LedgerEvent } from "../../types";

/** A question older than this is flagged as possibly expired; it is never closed for the user. */
export const STALE_AFTER_DAYS = 7;
const DAY_MS = 24 * 60 * 60 * 1000;

export type DecisionItem = {
  groupId: string;
  groupTitle: string;
  event: LedgerEvent;
  /** Whole days since it was asked; null when its time cannot be read. */
  ageDays: number | null;
  /** Asked more than STALE_AFTER_DAYS ago, so it may no longer matter. */
  stale: boolean;
};

export type DecisionGroupPage = {
  groupId: string;
  groupTitle: string;
  events: readonly LedgerEvent[];
};

function askedAt(event: LedgerEvent): number | null {
  const time = Date.parse(String(event.ts || ""));
  return Number.isFinite(time) ? time : null;
}

/**
 * Every Group's open questions as one queue. Questions still fresh come first, newest
 * first, since a recent question is the likeliest to be holding work up now; possibly
 * expired ones follow, oldest last. A question whose time cannot be read is treated as
 * fresh: it is never hidden behind the stale ones on a guess.
 */
export function buildDecisionQueue(
  pages: readonly DecisionGroupPage[],
  now: number,
): DecisionItem[] {
  const seen = new Set<string>();
  const items: DecisionItem[] = [];
  for (const page of pages) {
    for (const event of page.events) {
      const id = String(event.id || "");
      if (!id || seen.has(`${page.groupId}\u0000${id}`)) continue;
      seen.add(`${page.groupId}\u0000${id}`);
      const at = askedAt(event);
      const ageDays = at === null ? null : Math.max(0, Math.floor((now - at) / DAY_MS));
      items.push({
        groupId: page.groupId,
        groupTitle: page.groupTitle,
        event,
        ageDays,
        stale: ageDays !== null && ageDays >= STALE_AFTER_DAYS,
      });
    }
  }
  const time = (item: DecisionItem) => askedAt(item.event) ?? Number.POSITIVE_INFINITY;
  return items.sort((a, b) => Number(a.stale) - Number(b.stale) || time(b) - time(a));
}

export type DecisionFilter = "all" | "fresh" | "stale";

export function filterDecisions(items: readonly DecisionItem[], filter: DecisionFilter) {
  if (filter === "fresh") return items.filter((item) => !item.stale);
  if (filter === "stale") return items.filter((item) => item.stale);
  return [...items];
}
