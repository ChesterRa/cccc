import { describe, expect, it } from "vite-plus/test";
import { getVisualViewportLayout } from "./useViewportHeight";

describe("getVisualViewportLayout", () => {
  it("uses the visual viewport height without subtracting the keyboard twice", () => {
    expect(getVisualViewportLayout(512, 0, 0, 0)).toEqual({ height: "512px", offsetTop: "0px" });
  });

  it("follows the visual viewport when iOS pans it for the keyboard", () => {
    expect(getVisualViewportLayout(512, 684, 684, 0)).toEqual({
      height: "512px",
      offsetTop: "684px",
    });
  });

  it("falls back to pageTop when WebKit briefly reports a stale offsetTop", () => {
    expect(getVisualViewportLayout(512, 0, 684, 0)).toEqual({
      height: "512px",
      offsetTop: "684px",
    });
  });

  it("does not treat ordinary document scrolling as a visual viewport offset", () => {
    expect(getVisualViewportLayout(512, 0, 684, 684)).toEqual({
      height: "512px",
      offsetTop: "0px",
    });
  });

  it("ignores invalid viewport measurements", () => {
    expect(getVisualViewportLayout(0, 0, 0, 0)).toBeNull();
    expect(getVisualViewportLayout(Number.NaN, 0, 0, 0)).toBeNull();
  });
});
