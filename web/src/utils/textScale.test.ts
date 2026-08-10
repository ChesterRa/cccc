import { describe, expect, it } from "vite-plus/test";

import {
  DEFAULT_TEXT_SCALE,
  TEXT_SCALE_OPTIONS,
  getNextTextScale,
  normalizeTextScale,
} from "./textScale";

describe("text scale", () => {
  it("supports the complete ordered scale list", () => {
    expect(TEXT_SCALE_OPTIONS).toEqual([70, 90, 100, 125]);
    expect(TEXT_SCALE_OPTIONS.map(normalizeTextScale)).toEqual(TEXT_SCALE_OPTIONS);
  });

  it("falls back to the default for unsupported values", () => {
    expect(normalizeTextScale(80)).toBe(DEFAULT_TEXT_SCALE);
    expect(normalizeTextScale("invalid")).toBe(DEFAULT_TEXT_SCALE);
  });

  it("cycles through every scale and wraps to 70 percent", () => {
    expect(getNextTextScale(70)).toBe(90);
    expect(getNextTextScale(90)).toBe(100);
    expect(getNextTextScale(100)).toBe(125);
    expect(getNextTextScale(125)).toBe(70);
  });
});
