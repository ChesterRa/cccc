import { expect, it } from "vite-plus/test";
import { aggregatePendingIndicator, pendingFilterIndicator, pendingIndicator } from "./indicator";

it("never represents an unknown or stale count as a certain zero", () => {
  expect(pendingIndicator(undefined)?.text).toBe("?");
  expect(pendingIndicator({ state: "stale", pending: 0 })?.text).toBe("?");
  expect(pendingIndicator({ state: "stale", pending: 3 })?.text).toBe("~3");
  expect(pendingIndicator({ state: "fresh", pending: 0 })).toBeNull();
  expect(pendingIndicator({ state: "fresh", pending: 101 })?.text).toBe("99+");
});

it("keeps the active waiting filter at zero until the user leaves", () => {
  expect(pendingFilterIndicator({ state: "fresh", pending: 0 }, true)?.text).toBe("0");
  expect(pendingFilterIndicator({ state: "fresh", pending: 0 }, false)).toBeNull();
  expect(pendingFilterIndicator({ state: "fresh", pending: 2 }, false)?.text).toBe("2");
  expect(pendingFilterIndicator({ state: "stale", pending: 2 }, false)?.text).toBe("~2");
  expect(pendingFilterIndicator(undefined, false)?.text).toBe("?");
});

it("only sums known counts and keeps unknown Groups visible", () => {
  const summaries = {
    a: { state: "fresh" as const, pending: 2 },
    b: { state: "unknown" as const, pending: null },
  };
  expect(aggregatePendingIndicator(summaries, ["a", "b"])?.text).toBe("2+?");
  expect(aggregatePendingIndicator(summaries, ["b"])?.text).toBe("?");
  expect(aggregatePendingIndicator(summaries, ["a", "a"])?.text).toBe("2");
  expect(aggregatePendingIndicator(summaries, [])).toBeNull();
});

it("says when the running daemon cannot give a count, still without a zero", () => {
  const unsupported = { state: "unknown" as const, pending: null, daemonUnsupported: true };
  expect(pendingIndicator(unsupported)).toEqual({
    text: "?",
    state: "unknown",
    count: null,
    daemonUnsupported: true,
  });
  expect(pendingIndicator({ state: "unknown", pending: null })?.daemonUnsupported).toBeUndefined();
  expect(pendingFilterIndicator(unsupported, true)?.text).toBe("?");
  const summaries = { a: { state: "fresh" as const, pending: 2 }, b: unsupported };
  expect(aggregatePendingIndicator(summaries, ["a", "b"])).toEqual({
    text: "2+?",
    state: "unknown",
    count: null,
    daemonUnsupported: true,
  });
  expect(aggregatePendingIndicator(summaries, ["a"])?.daemonUnsupported).toBeUndefined();
});
