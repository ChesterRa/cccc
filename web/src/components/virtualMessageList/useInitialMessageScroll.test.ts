import { describe, expect, it } from "vite-plus/test";

import { shouldAcceptLateScrollRestoration } from "./useInitialMessageScroll";

describe("shouldAcceptLateScrollRestoration", () => {
  it("accepts a late anchor only when the view initially had no restoration target", () => {
    expect(
      shouldAcceptLateScrollRestoration({
        previousRequestKey: "bottom",
        nextRequestKey: "anchor:event-1:20",
        reentryDeadline: 2_000,
        now: 1_000,
      }),
    ).toBe(true);
  });

  it("rejects restoration snapshots produced by an in-progress anchor restoration", () => {
    expect(
      shouldAcceptLateScrollRestoration({
        previousRequestKey: "anchor:event-1:20",
        nextRequestKey: "anchor:event-2:40",
        reentryDeadline: 2_000,
        now: 1_000,
      }),
    ).toBe(false);
  });

  it("rejects late anchors after the handoff window", () => {
    expect(
      shouldAcceptLateScrollRestoration({
        previousRequestKey: "bottom",
        nextRequestKey: "anchor:event-1:20",
        reentryDeadline: 2_000,
        now: 2_001,
      }),
    ).toBe(false);
  });
});
