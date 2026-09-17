import { describe, expect, it } from "vite-plus/test";

import { isChatViewportAtBottom } from "../../src/hooks/useAppTabState";

describe("useAppTabState", () => {
  it("treats only the real viewport position as at-bottom", () => {
    expect(isChatViewportAtBottom(1000, 920, 80)).toBe(true);
    expect(isChatViewportAtBottom(1000, 700, 80)).toBe(false);
  });

  it("respects the bottom threshold without any external follow-mode state", () => {
    expect(isChatViewportAtBottom(1000, 821, 80, 100)).toBe(true);
    expect(isChatViewportAtBottom(1000, 820, 80, 100)).toBe(false);
  });
});
