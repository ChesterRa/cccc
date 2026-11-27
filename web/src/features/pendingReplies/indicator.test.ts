import { expect, it } from "vite-plus/test";
import {
  aggregatePendingIndicator,
  pendingFilterIndicator,
  pendingIndicator,
  pendingIndicatorStatusLabel,
  pendingIndicatorSummaryLabel,
} from "./indicator";

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

it("distinguishes needs-user, Chet-handling, and unknown unattended states without counting unknown as user work", () => {
  const withChetOnly = {
    state: "fresh" as const,
    pending: 2,
    unattended: { state: "fresh" as const, needsUser: 0, withChet: 2, unknown: 0 },
  };
  expect(pendingIndicator(withChetOnly)).toMatchObject({
    text: "2",
    state: "fresh",
    tone: "chet",
    withChetCount: 2,
  });
  expect(pendingFilterIndicator(withChetOnly, false)).toMatchObject({ text: "2", tone: "chet" });

  const handedBack = {
    state: "fresh" as const,
    pending: 3,
    unattended: { state: "fresh" as const, needsUser: 1, withChet: 2, unknown: 0 },
  };
  expect(pendingIndicator(handedBack)).toMatchObject({
    text: "1",
    state: "fresh",
    tone: "attention",
    needsUserCount: 1,
    withChetCount: 2,
  });

  const userPlusUnknown = {
    state: "fresh" as const,
    pending: 3,
    unattended: { state: "fresh" as const, needsUser: 1, withChet: 1, unknown: 1 },
  };
  expect(pendingIndicator(userPlusUnknown)).toMatchObject({
    text: "1+?",
    state: "fresh",
    tone: "attention",
    needsUserCount: 1,
    unknownCount: 1,
  });

  const unknownOnly = {
    state: "fresh" as const,
    pending: 2,
    unattended: { state: "fresh" as const, needsUser: 0, withChet: 1, unknown: 1 },
  };
  expect(pendingIndicator(unknownOnly)).toMatchObject({
    text: "?",
    state: "unknown",
    count: null,
    tone: "unknown",
  });

  const breakdownUnknown = {
    state: "fresh" as const,
    pending: 4,
    unattended: { state: "unknown" as const, needsUser: null, withChet: null, unknown: null },
  };
  expect(pendingIndicator(breakdownUnknown)).toMatchObject({
    text: "?",
    state: "unknown",
    count: null,
    tone: "unknown",
  });
});

it("keeps all four stale unattended quadrants subdued when no user action was confirmed", () => {
  expect(
    pendingIndicator({
      state: "stale",
      pending: 2,
      unattended: { state: "fresh", needsUser: 2, withChet: 0, unknown: 0 },
    }),
  ).toMatchObject({ text: "~2", state: "stale", tone: "attention" });
  expect(
    pendingIndicator({
      state: "stale",
      pending: 3,
      unattended: { state: "fresh", needsUser: 1, withChet: 1, unknown: 1 },
    }),
  ).toMatchObject({ text: "~1+?", state: "stale", tone: "attention" });
  expect(
    pendingIndicator({
      state: "stale",
      pending: 2,
      unattended: { state: "fresh", needsUser: 0, withChet: 1, unknown: 1 },
    }),
  ).toMatchObject({ text: "?", state: "stale", tone: "unknown" });
  expect(
    pendingIndicator({
      state: "stale",
      pending: 2,
      unattended: { state: "fresh", needsUser: 0, withChet: 2, unknown: 0 },
    }),
  ).toMatchObject({ text: "~2", state: "stale", tone: "chet" });
  expect(
    pendingIndicator({
      state: "stale",
      pending: 2,
      unattended: { state: "unknown", needsUser: null, withChet: null, unknown: null },
    }),
  ).toMatchObject({ text: "?", state: "stale", tone: "unknown" });
});

it("aggregates unattended Groups without counting unknown breakdown items as needs-user", () => {
  const summaries = {
    chetOnly: {
      state: "fresh" as const,
      pending: 2,
      unattended: { state: "fresh" as const, needsUser: 0, withChet: 2, unknown: 0 },
    },
    unknownBreakdown: {
      state: "fresh" as const,
      pending: 5,
      unattended: { state: "unknown" as const, needsUser: null, withChet: null, unknown: null },
    },
    needsUser: {
      state: "fresh" as const,
      pending: 3,
      unattended: { state: "fresh" as const, needsUser: 1, withChet: 2, unknown: 0 },
    },
    legacy: { state: "fresh" as const, pending: 2 },
  };
  expect(aggregatePendingIndicator(summaries, ["chetOnly"])).toMatchObject({
    text: "2",
    tone: "chet",
  });
  expect(aggregatePendingIndicator(summaries, ["chetOnly", "unknownBreakdown"])).toMatchObject({
    text: "?",
    tone: "unknown",
    needsUserCount: 0,
  });
  expect(aggregatePendingIndicator(summaries, ["needsUser", "unknownBreakdown"])).toMatchObject({
    text: "1+?",
    tone: "attention",
    needsUserCount: 1,
  });
  expect(aggregatePendingIndicator(summaries, ["legacy", "chetOnly"])).toMatchObject({
    text: "2",
    tone: "attention",
    needsUserCount: 2,
    withChetCount: 2,
  });
});

it("keeps aggregate outer-unknown and stale summaries in attention tone with original labels under unattended mode", () => {
  const staleSummaries = {
    staleGroup: {
      state: "stale" as const,
      pending: 2,
      unattended: { state: "fresh" as const, needsUser: 2, withChet: 0, unknown: 0 },
    },
    quietGroup: {
      state: "fresh" as const,
      pending: 0,
      unattended: { state: "fresh" as const, needsUser: 0, withChet: 0, unknown: 0 },
    },
  };
  const aggregate = aggregatePendingIndicator(staleSummaries, ["staleGroup", "quietGroup"])!;
  expect(aggregate).toEqual({ text: "?", state: "unknown", count: null, tone: "attention" });
  const t = (key: string, opts?: Record<string, unknown>) =>
    opts ? `${key}:${JSON.stringify(opts)}` : key;
  expect(pendingIndicatorStatusLabel(t, aggregate)).toBe("pendingRepliesUnknown");
  expect(pendingIndicatorSummaryLabel(t, aggregate)).toBe('pendingRepliesIndicator:{"value":"?"}');

  // Only breakdown-level uncertainty (outer fresh) stays subdued.
  const breakdownOnly = aggregatePendingIndicator(
    {
      a: {
        state: "fresh",
        pending: 1,
        unattended: { state: "unknown", needsUser: null, withChet: null, unknown: null },
      },
      b: staleSummaries.quietGroup,
    },
    ["a", "b"],
  )!;
  expect(breakdownOnly).toMatchObject({ text: "?", state: "unknown", tone: "unknown" });
  expect(pendingIndicatorSummaryLabel(t, breakdownOnly)).toBe("pendingRepliesUnattendedUnknown");
});
