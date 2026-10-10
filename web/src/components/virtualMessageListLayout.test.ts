import { describe, expect, it } from "vite-plus/test";

import { getNonVirtualMessageListTopMargin } from "./virtualMessageListLayout";

describe("getNonVirtualMessageListTopMargin", () => {
  it("always reserves space for the history status badge before the first short-list message", () => {
    // The badge toggles with loading/exhausted state. The scroll container
    // disables overflow anchoring, so a margin that followed the badge would
    // shift every row by the badge height when a history load starts.
    expect(getNonVirtualMessageListTopMargin({ topInset: 0 })).toBe(56);
    expect(getNonVirtualMessageListTopMargin({ topInset: 24 })).toBe(80);
  });

  it("ignores invalid top insets", () => {
    expect(getNonVirtualMessageListTopMargin({ topInset: -10 })).toBe(56);
    expect(getNonVirtualMessageListTopMargin({ topInset: Number.NaN })).toBe(56);
  });
});
