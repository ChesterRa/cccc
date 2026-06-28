import { describe, expect, it } from "vitest";

import { getNonVirtualMessageListTopMargin } from "./virtualMessageListLayout";

describe("getNonVirtualMessageListTopMargin", () => {
  it("reserves space for the exhausted-history badge before the first short-list message", () => {
    expect(getNonVirtualMessageListTopMargin({ topInset: 0, showHistoryStatus: true })).toBe(56);
    expect(getNonVirtualMessageListTopMargin({ topInset: 24, showHistoryStatus: true })).toBe(80);
  });

  it("keeps short-list messages at the regular top inset when no history status is visible", () => {
    expect(getNonVirtualMessageListTopMargin({ topInset: 0, showHistoryStatus: false })).toBe(0);
    expect(getNonVirtualMessageListTopMargin({ topInset: 24, showHistoryStatus: false })).toBe(24);
  });
});
