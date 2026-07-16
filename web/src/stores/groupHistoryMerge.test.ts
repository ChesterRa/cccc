import { describe, expect, it } from "vite-plus/test";
import type { LedgerEvent } from "../types";
import { mergeOlderLedgerEvents } from "./groupHistoryMerge";

function event(id: string): LedgerEvent {
  return { id, kind: "chat.message", data: { text: id } };
}

describe("mergeOlderLedgerEvents", () => {
  it("keeps both loaded history and the newest messages beyond the old 800-event cap", () => {
    const current = Array.from({ length: 800 }, (_, index) => event(`current-${index}`));
    const older = Array.from({ length: 50 }, (_, index) => event(`older-${index}`));

    const result = mergeOlderLedgerEvents(current, older);

    expect(result.added).toBe(50);
    expect(result.events).toHaveLength(850);
    expect(result.events[0]?.id).toBe("older-0");
    expect(result.events.at(-1)?.id).toBe("current-799");
  });

  it("deduplicates repeated history pages", () => {
    const result = mergeOlderLedgerEvents([event("2")], [event("1"), event("2"), event("1")]);
    expect(result.events.map((item) => item.id)).toEqual(["1", "2"]);
    expect(result.added).toBe(1);
  });
});
