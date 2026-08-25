import { describe, expect, it } from "vite-plus/test";

import { getTerminalTheme } from "./terminalTheme";

describe("getTerminalTheme", () => {
  it("uses one dark PTY palette independent of the dashboard theme", () => {
    const theme = getTerminalTheme();

    expect(theme.background).toBe("#0f172a");
    expect(theme.foreground).toBe("#e2e8f0");
    expect(theme.cursorAccent).toBe(theme.background);
    expect(theme.black).not.toBe(theme.background);
  });
});
