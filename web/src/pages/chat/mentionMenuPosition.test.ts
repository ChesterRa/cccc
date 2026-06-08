import { describe, expect, it } from "vitest";

import { getMentionMenuLeft } from "./mentionMenuPosition";

describe("getMentionMenuLeft", () => {
  it("anchors the menu near the current @ trigger instead of the composer start", () => {
    expect(getMentionMenuLeft({ triggerX: 720, containerWidth: 900, menuWidth: 320 })).toBe(564);
  });

  it("keeps the menu inside the composer when the trigger is near the left edge", () => {
    expect(getMentionMenuLeft({ triggerX: 4, containerWidth: 900, menuWidth: 320 })).toBe(8);
  });

  it("keeps the menu inside the composer when the trigger is near the right edge", () => {
    expect(getMentionMenuLeft({ triggerX: 880, containerWidth: 900, menuWidth: 320 })).toBe(572);
  });
});
