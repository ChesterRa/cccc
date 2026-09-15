import { describe, expect, it } from "vite-plus/test";
import { clampSidePanelWidth, shouldCompactSidePanel } from "../../src/utils/sidePanelLayout";

describe("side panel sizing", () => {
  it("keeps a useful default and leaves room for the main work area", () => {
    expect(clampSidePanelWidth(NaN)).toBe(360);
    expect(clampSidePanelWidth(0)).toBe(360);
    expect(clampSidePanelWidth(100)).toBe(280);
    expect(clampSidePanelWidth(780, 1000)).toBe(672);
    expect(clampSidePanelWidth(360, 520)).toBe(192);
  });
  it("does not bounce between compact and expanded at one threshold", () => {
    expect(shouldCompactSidePanel(179, false)).toBe(true);
    expect(shouldCompactSidePanel(200, true)).toBe(true);
    expect(shouldCompactSidePanel(221, true)).toBe(false);
    expect(shouldCompactSidePanel(200, false)).toBe(false);
  });
});
