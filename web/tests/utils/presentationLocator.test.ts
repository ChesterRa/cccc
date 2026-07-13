import { describe, expect, it } from "vite-plus/test";
import {
  canRestorePresentationRefInViewer,
  getPresentationRefViewerScrollTop,
  shouldAutoOpenInteractivePresentation,
} from "../../src/utils/presentationLocator";

describe("presentation locator utils", () => {
  it("restores viewer scroll only for stable in-view content types", () => {
    expect(canRestorePresentationRefInViewer("markdown")).toBe(true);
    expect(canRestorePresentationRefInViewer("table")).toBe(true);
    expect(canRestorePresentationRefInViewer("pdf")).toBe(false);
    expect(canRestorePresentationRefInViewer("web_preview")).toBe(false);
    expect(canRestorePresentationRefInViewer("image")).toBe(false);
  });

  it("reads viewer_scroll_top from a presentation ref locator conservatively", () => {
    expect(
      getPresentationRefViewerScrollTop({
        kind: "presentation_ref",
        slot_id: "slot-2",
        locator: { viewer_scroll_top: 240 },
      }),
    ).toBe(240);
    expect(
      getPresentationRefViewerScrollTop({
        kind: "presentation_ref",
        slot_id: "slot-2",
        locator: { viewer_scroll_top: "128" },
      }),
    ).toBe(128);
    expect(
      getPresentationRefViewerScrollTop({
        kind: "presentation_ref",
        slot_id: "slot-2",
        locator: { viewer_scroll_top: -1 },
      }),
    ).toBeNull();
    expect(
      getPresentationRefViewerScrollTop({
        kind: "presentation_ref",
        slot_id: "slot-2",
        locator: { viewer_scroll_top: "bad" },
      }),
    ).toBeNull();
    expect(
      getPresentationRefViewerScrollTop({ kind: "presentation_ref", slot_id: "slot-2" }),
    ).toBeNull();
  });

  it("auto-opens interactive mode only when a live browser session is already active", () => {
    expect(shouldAutoOpenInteractivePresentation(true, { active: true, state: "ready" })).toBe(
      true,
    );
    expect(shouldAutoOpenInteractivePresentation(true, { active: true, state: "starting" })).toBe(
      true,
    );
    expect(shouldAutoOpenInteractivePresentation(true, { active: true, state: "failed" })).toBe(
      false,
    );
    expect(shouldAutoOpenInteractivePresentation(true, { active: false, state: "ready" })).toBe(
      false,
    );
    expect(shouldAutoOpenInteractivePresentation(false, { active: true, state: "ready" })).toBe(
      false,
    );
  });
});
