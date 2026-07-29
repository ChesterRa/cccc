import { describe, expect, it } from "vite-plus/test";

import {
  getInitialAnchorCorrection,
  getMessageAnchorOffset,
  getScrollOffsetForMessageAnchor,
} from "./virtualMessageListAnchorRestore";

describe("virtual message anchor offset", () => {
  it("preserves the signed top-inset offset for the first message", () => {
    const offsetPx = getMessageAnchorOffset(0, 72);

    expect(offsetPx).toBe(-72);
    expect(getScrollOffsetForMessageAnchor(72, offsetPx)).toBe(0);
  });

  it("round-trips an anchor partially scrolled above the viewport", () => {
    const offsetPx = getMessageAnchorOffset(640, 520);

    expect(offsetPx).toBe(120);
    expect(getScrollOffsetForMessageAnchor(520, offsetPx)).toBe(640);
  });
});

describe("initial virtual message anchor correction", () => {
  it("keeps the restored message at the same viewport position after delayed row measurement", () => {
    expect(
      getInitialAnchorCorrection({
        currentScrollTop: 1_200,
        lockedAnchorTop: 80,
        currentAnchorTop: 260,
        now: 500,
        expiresAt: 2_000,
      }),
    ).toEqual({ active: true, scrollTop: 1_380 });
  });

  it("ignores sub-pixel measurement noise", () => {
    expect(
      getInitialAnchorCorrection({
        currentScrollTop: 1_200,
        lockedAnchorTop: 80,
        currentAnchorTop: 80.4,
        now: 500,
        expiresAt: 2_000,
      }),
    ).toEqual({ active: true, scrollTop: 1_200 });
  });

  it("stops correcting after the bounded restore window", () => {
    expect(
      getInitialAnchorCorrection({
        currentScrollTop: 1_200,
        lockedAnchorTop: 80,
        currentAnchorTop: 260,
        now: 2_001,
        expiresAt: 2_000,
      }),
    ).toEqual({ active: false, scrollTop: 1_200 });
  });
});
