import { describe, expect, it } from "vite-plus/test";
import { getVisualViewportHeight } from "./useViewportHeight";

describe("getVisualViewportHeight", () => {
  it("uses the visual viewport height without subtracting the keyboard twice", () => {
    expect(getVisualViewportHeight(512)).toBe("512px");
  });

  it("ignores invalid viewport measurements", () => {
    expect(getVisualViewportHeight(0)).toBeNull();
    expect(getVisualViewportHeight(Number.NaN)).toBeNull();
  });
});
