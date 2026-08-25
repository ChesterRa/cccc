import { describe, expect, it } from "vite-plus/test";

import { getAutomaticTheme, millisecondsUntilNextThemeBoundary } from "../../src/utils/theme";

describe("automatic theme", () => {
  it("uses the browser-local daytime window for the light theme", () => {
    expect(getAutomaticTheme(new Date(2026, 7, 24, 6, 59))).toBe("dark");
    expect(getAutomaticTheme(new Date(2026, 7, 24, 7, 0))).toBe("light");
    expect(getAutomaticTheme(new Date(2026, 7, 24, 18, 59))).toBe("light");
    expect(getAutomaticTheme(new Date(2026, 7, 24, 19, 0))).toBe("dark");
  });

  it("schedules the next local-time boundary", () => {
    expect(millisecondsUntilNextThemeBoundary(new Date(2026, 7, 24, 6, 30))).toBe(30 * 60 * 1_000);
    expect(millisecondsUntilNextThemeBoundary(new Date(2026, 7, 24, 18, 30))).toBe(30 * 60 * 1_000);
    expect(millisecondsUntilNextThemeBoundary(new Date(2026, 7, 24, 19, 0))).toBe(
      12 * 60 * 60 * 1_000,
    );
  });
});
