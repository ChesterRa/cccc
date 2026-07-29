import { describe, expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";

import { getScrollRestorationRequestKey } from "./useScrollAnchorRestoration";

describe("getScrollRestorationRequestKey", () => {
  it("distinguishes a late anchor handoff from an initial bottom fallback", () => {
    expect(getScrollRestorationRequestKey({})).toBe("bottom");
    expect(getScrollRestorationRequestKey({ anchorId: "event-1", offsetPx: 24 })).toBe(
      "anchor:event-1:24",
    );
  });

  it("rounds sub-pixel measurement noise without hiding meaningful movement", () => {
    expect(getScrollRestorationRequestKey({ anchorId: "event-1", offsetPx: 24.12 })).toBe(
      "anchor:event-1:24",
    );
    expect(getScrollRestorationRequestKey({ anchorId: "event-1", offsetPx: 25 })).toBe(
      "anchor:event-1:25",
    );
  });

  it("keeps signed offsets that place the first row below the viewport top", () => {
    expect(getScrollRestorationRequestKey({ anchorId: "event-1", offsetPx: -72 })).toBe(
      "anchor:event-1:-72",
    );
  });

  it("keeps explicit deep-link targets separate from restored anchors", () => {
    expect(
      getScrollRestorationRequestKey({
        targetId: "target-event",
        anchorId: "anchor-event",
        offsetPx: 10,
      }),
    ).toBe("target:target-event");
  });
});

describe("scroll anchor correction lifecycle", () => {
  it("keeps checking for virtual measurement drift during the restoration window", () => {
    const source = readFileSync(
      new URL("./useScrollAnchorRestoration.ts", import.meta.url),
      "utf8",
    );
    const correctionSchedules = source.match(/requestAnimationFrame\(runCorrection\)/g) || [];
    expect(correctionSchedules).toHaveLength(2);
  });
});
