import { describe, expect, it } from "vite-plus/test";

import { settingsDialogFooterClass } from "./types";

describe("settingsDialogFooterClass", () => {
  it("keeps base bottom padding while adding the device safe area", () => {
    expect(settingsDialogFooterClass).toContain("pb-[calc(1rem+env(safe-area-inset-bottom,0px))]");
    expect(settingsDialogFooterClass).toContain(
      "sm:pb-[calc(1.25rem+env(safe-area-inset-bottom,0px))]",
    );
    expect(settingsDialogFooterClass).not.toContain("safe-area-bottom-compact");
  });
});
