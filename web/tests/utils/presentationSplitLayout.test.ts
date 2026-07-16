import { describe, expect, it } from "vite-plus/test";

import {
  PRESENTATION_SPLIT_DEFAULT_WIDTH,
  PRESENTATION_SPLIT_MIN_VIEWER_WIDTH,
  PRESENTATION_SPLIT_RAIL_WIDTH,
  clampPresentationSplitWidth,
} from "../../src/utils/presentationSplitLayout";

describe("presentationSplitLayout", () => {
  const minWidth = PRESENTATION_SPLIT_RAIL_WIDTH + PRESENTATION_SPLIT_MIN_VIEWER_WIDTH;

  it("falls back to the default width for invalid values", () => {
    expect(clampPresentationSplitWidth(Number.NaN)).toBe(PRESENTATION_SPLIT_DEFAULT_WIDTH);
  });

  it("enforces the structural minimum width", () => {
    expect(clampPresentationSplitWidth(120)).toBe(minWidth);
  });

  it("respects the available container width while preserving chat minimum width", () => {
    expect(clampPresentationSplitWidth(640, 760)).toBe(444);
    expect(clampPresentationSplitWidth(640, 1200)).toBe(640);
    expect(clampPresentationSplitWidth(900, 1200)).toBe(884);
  });

  it("does not impose an artificial hard cap on wide layouts", () => {
    expect(clampPresentationSplitWidth(1400, 2000)).toBe(1400);
    expect(clampPresentationSplitWidth(1900, 2000)).toBe(1684);
  });
});
