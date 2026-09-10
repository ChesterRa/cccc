import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
  clampComposerHeight,
  composerHeightLimit,
  loadComposerHeight,
  saveComposerHeight,
} from "./composerHeight";

afterEach(() => vi.unstubAllGlobals());
describe("composer height limits", () => {
  it("clamps invalid and out-of-range preferences", () => {
    expect(clampComposerHeight(NaN, 300)).toBe(64);
    expect(clampComposerHeight(10, 300)).toBe(64);
    expect(clampComposerHeight(999, 300)).toBe(300);
    expect(clampComposerHeight(201.6, 300)).toBe(202);
    expect(clampComposerHeight(200, 20)).toBe(64);
  });
  it("reclamps to a smaller panel and keeps room for messages and controls", () => {
    expect(composerHeightLimit(800, 120, 1)).toBe(480);
    expect(composerHeightLimit(300, 100, 1)).toBe(120);
    expect(clampComposerHeight(400, composerHeightLimit(300, 100, 1))).toBe(120);
    expect(composerHeightLimit(800, 150, 1.25)).toBe(384);
  });
  it("survives storage read and write failures", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw Error("blocked");
      },
      setItem: () => {
        throw Error("blocked");
      },
    });
    expect(loadComposerHeight()).toBe(64);
    expect(() => saveComposerHeight(300)).not.toThrow();
  });
});
