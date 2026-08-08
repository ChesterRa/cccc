import { describe, expect, it } from "vite-plus/test";
import { mapContainedImagePoint } from "./projectedBrowserCoordinates";
import {
  projectedBrowserEffectiveViewerMode,
  projectedBrowserSocketViewerMode,
  shouldReuseProjectedBrowserSession,
} from "./projectedBrowserViewerMode";

describe("mapContainedImagePoint", () => {
  it("removes object-contain letterboxing before mapping browser coordinates", () => {
    expect(
      mapContainedImagePoint(
        { x: 500, y: 300 },
        { left: 0, top: 0, width: 1000, height: 600 },
        { width: 1000, height: 500 },
      ),
    ).toEqual({ x: 500, y: 250 });
  });

  it("ignores clicks in the letterbox instead of clicking the page edge", () => {
    expect(
      mapContainedImagePoint(
        { x: 500, y: 20 },
        { left: 0, top: 0, width: 1000, height: 600 },
        { width: 1000, height: 500 },
      ),
    ).toBeNull();
  });
});

describe("projected browser viewer modes", () => {
  it("forces page projection without changing the browser session", () => {
    expect(projectedBrowserSocketViewerMode("page", false)).toBe("screencast");
    expect(projectedBrowserEffectiveViewerMode("page", true, false)).toBe("page");
  });

  it("uses the complete browser transport only when VNC is available", () => {
    expect(projectedBrowserSocketViewerMode("browser", false)).toBe("auto");
    expect(projectedBrowserEffectiveViewerMode("browser", true, false)).toBe("browser");
    expect(projectedBrowserEffectiveViewerMode("browser", false, false)).toBe("page");
  });

  it("falls back to page projection after a VNC disconnect", () => {
    expect(projectedBrowserSocketViewerMode("browser", true)).toBe("screencast");
    expect(projectedBrowserEffectiveViewerMode("browser", true, true)).toBe("page");
  });

  it("reuses the active session for a viewer-only reconnect", () => {
    expect(shouldReuseProjectedBrowserSession(false, "same", "same")).toBe(true);
    expect(shouldReuseProjectedBrowserSession(false, "old", "new")).toBe(false);
    expect(shouldReuseProjectedBrowserSession(true, "old", "new")).toBe(true);
  });
});
