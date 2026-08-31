import { describe, expect, it } from "vite-plus/test";

import { canReorderSidebarGroups, groupSidebarScrollClass } from "./groupSidebarModel";

describe("canReorderSidebarGroups", () => {
  it("keeps touch scrolling exclusive on small screens", () => {
    expect(
      canReorderSidebarGroups({ isSmallScreen: true, isCollapsed: false, readOnly: false }),
    ).toBe(false);
  });

  it("enables reordering only for an expanded writable desktop sidebar", () => {
    expect(
      canReorderSidebarGroups({ isSmallScreen: false, isCollapsed: false, readOnly: false }),
    ).toBe(true);
    expect(
      canReorderSidebarGroups({ isSmallScreen: false, isCollapsed: true, readOnly: false }),
    ).toBe(false);
    expect(
      canReorderSidebarGroups({ isSmallScreen: false, isCollapsed: false, readOnly: true }),
    ).toBe(false);
  });

  it("keeps touch scrolling and safe-area padding on the scroll region", () => {
    expect(groupSidebarScrollClass(false)).toContain("touch-pan-y");
    expect(groupSidebarScrollClass(false)).toContain("safe-area-inset-bottom");
    expect(groupSidebarScrollClass(true)).toContain("pb-[calc(0.5rem+");
  });
});
