import { describe, expect, it } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { buildDecisionQueue, filterDecisions, STALE_AFTER_DAYS } from "./decisionQueueModel";

const NOW = Date.parse("2026-11-28T00:00:00Z");
const DAY = 24 * 60 * 60 * 1000;
const asked = (id: string, daysAgo: number | null): LedgerEvent => ({
  id,
  kind: "chat.message",
  by: "worker",
  ts: daysAgo === null ? "not a time" : new Date(NOW - daysAgo * DAY).toISOString(),
  data: { text: id },
});
const page = (groupId: string, events: LedgerEvent[]) => ({
  groupId,
  groupTitle: groupId.toUpperCase(),
  events,
});
const ids = (items: { event: LedgerEvent }[]) => items.map((item) => item.event.id);

describe("the decision queue", () => {
  it("puts fresh questions first, newest first, and possibly expired ones after", () => {
    const queue = buildDecisionQueue(
      [
        page("a", [asked("old", 30), asked("yesterday", 1)]),
        page("b", [asked("today", 0), asked("week", 9)]),
      ],
      NOW,
    );
    expect(ids(queue)).toEqual(["today", "yesterday", "week", "old"]);
    expect(queue.map((item) => item.stale)).toEqual([false, false, true, true]);
    expect(queue[0]).toMatchObject({ groupId: "b", groupTitle: "B", ageDays: 0 });
  });

  it(`marks a question stale from ${STALE_AFTER_DAYS} whole days, not before`, () => {
    const queue = buildDecisionQueue(
      [page("a", [asked("six", STALE_AFTER_DAYS - 0.01), asked("seven", STALE_AFTER_DAYS)])],
      NOW,
    );
    expect(queue.find((item) => item.event.id === "six")?.stale).toBe(false);
    expect(queue.find((item) => item.event.id === "seven")?.stale).toBe(true);
  });

  it("never hides a question whose time cannot be read behind the stale ones", () => {
    const [first] = buildDecisionQueue(
      [page("a", [asked("old", 40), asked("undated", null)])],
      NOW,
    );
    expect(first).toMatchObject({ stale: false, ageDays: null });
    expect(first!.event.id).toBe("undated");
  });

  it("lists a question once per Group, and keeps the same ID apart across Groups", () => {
    const queue = buildDecisionQueue(
      [page("a", [asked("q", 1), asked("q", 1)]), page("b", [asked("q", 2)])],
      NOW,
    );
    expect(queue.map((item) => item.groupId)).toEqual(["a", "b"]);
  });

  it("filters by what still needs deciding and what may have expired", () => {
    const queue = buildDecisionQueue([page("a", [asked("new", 1), asked("old", 20)])], NOW);
    expect(ids(filterDecisions(queue, "fresh"))).toEqual(["new"]);
    expect(ids(filterDecisions(queue, "stale"))).toEqual(["old"]);
    expect(filterDecisions(queue, "all")).toHaveLength(2);
  });
});
