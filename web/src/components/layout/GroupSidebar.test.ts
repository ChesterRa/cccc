import { describe, expect, it } from "vite-plus/test";

import {
  getSidebarReorderActivation,
  getSidebarSensorActivationConstraints,
  groupSidebarScrollClass,
} from "./groupSidebarModel";

describe("getSidebarReorderActivation", () => {
  it("activates from the row on every writable expanded sidebar", () => {
    expect(getSidebarReorderActivation({ isCollapsed: false, readOnly: false })).toBe("row");
    expect(getSidebarReorderActivation({ isCollapsed: false })).toBe("row");
  });

  it("disables reordering when collapsed or read-only", () => {
    expect(getSidebarReorderActivation({ isCollapsed: true, readOnly: false })).toBe("disabled");
    expect(getSidebarReorderActivation({ isCollapsed: false, readOnly: true })).toBe("disabled");
  });

  it("gives touch a long press so a scroll gesture never starts a drag", () => {
    expect(getSidebarSensorActivationConstraints()).toEqual({
      mouse: { distance: 4 },
      touch: { delay: 250, tolerance: 8 },
    });
  });

  it("keeps touch scrolling and safe-area padding on the scroll region", () => {
    expect(groupSidebarScrollClass(false)).toContain("touch-pan-y");
    expect(groupSidebarScrollClass(false)).toContain("safe-area-inset-bottom");
    expect(groupSidebarScrollClass(true)).toContain("pb-[calc(0.5rem+");
  });
});
