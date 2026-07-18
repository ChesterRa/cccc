import { describe, expect, it } from "vite-plus/test";

import {
  getCorrectedScrollTopForAnchor,
  getTopHistoryLoadDecision,
  shouldRearmTopHistoryLoad,
  shouldTriggerTopHistoryLoad,
} from "../../src/components/virtualMessageListPrependCompensation";

describe("virtualMessageListPrependCompensation", () => {
  it("does not trigger another history load while prepend compensation is active", () => {
    expect(
      shouldTriggerTopHistoryLoad({
        scrollTop: 12,
        topTriggerPx: 80,
        topLoadArmed: true,
        hasMoreHistory: true,
        isLoadingHistory: false,
        isPrependCompensating: true,
        hasLoadMoreHandler: true,
      }),
    ).toBe(false);
  });

  it("triggers top history load only when armed, idle, and inside the top threshold", () => {
    expect(
      shouldTriggerTopHistoryLoad({
        scrollTop: 79,
        topTriggerPx: 80,
        topLoadArmed: true,
        hasMoreHistory: true,
        isLoadingHistory: false,
        isPrependCompensating: false,
        hasLoadMoreHandler: true,
      }),
    ).toBe(true);
    expect(
      shouldTriggerTopHistoryLoad({
        scrollTop: 80,
        topTriggerPx: 80,
        topLoadArmed: true,
        hasMoreHistory: true,
        isLoadingHistory: false,
        isPrependCompensating: false,
        hasLoadMoreHandler: true,
      }),
    ).toBe(false);
  });

  it("keeps top history load disarmed after triggering until the viewport leaves the top band", () => {
    expect(
      getTopHistoryLoadDecision({
        scrollTop: 12,
        topTriggerPx: 80,
        topRearmPx: 240,
        topLoadArmed: true,
        hasMoreHistory: true,
        isLoadingHistory: false,
        isPrependCompensating: false,
        hasLoadMoreHandler: true,
      }),
    ).toEqual({ topLoadArmed: false, shouldLoad: true });

    expect(
      getTopHistoryLoadDecision({
        scrollTop: 120,
        topTriggerPx: 80,
        topRearmPx: 240,
        topLoadArmed: false,
        hasMoreHistory: true,
        isLoadingHistory: false,
        isPrependCompensating: false,
        hasLoadMoreHandler: true,
      }),
    ).toEqual({ topLoadArmed: false, shouldLoad: false });

    expect(
      getTopHistoryLoadDecision({
        scrollTop: 320,
        topTriggerPx: 80,
        topRearmPx: 240,
        topLoadArmed: false,
        hasMoreHistory: true,
        isLoadingHistory: false,
        isPrependCompensating: false,
        hasLoadMoreHandler: true,
      }),
    ).toEqual({ topLoadArmed: true, shouldLoad: false });
  });

  it("does not rearm top history loading during prepend compensation", () => {
    expect(
      shouldRearmTopHistoryLoad({ scrollTop: 320, topRearmPx: 240, isPrependCompensating: true }),
    ).toBe(false);
    expect(
      shouldRearmTopHistoryLoad({ scrollTop: 320, topRearmPx: 240, isPrependCompensating: false }),
    ).toBe(true);
  });

  it("computes DOM delta correction from the locked anchor top", () => {
    expect(
      getCorrectedScrollTopForAnchor({
        currentScrollTop: 120,
        lockedAnchorTop: 200,
        currentAnchorTop: 236,
      }),
    ).toBe(156);
    expect(
      getCorrectedScrollTopForAnchor({
        currentScrollTop: 120,
        lockedAnchorTop: 200,
        currentAnchorTop: 200.25,
        minDeltaPx: 0.5,
      }),
    ).toBe(120);
  });
});
