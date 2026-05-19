import { describe, expect, it } from "vitest";

import {
  filterTerminalWorkingBannerChunk,
  getTerminalSignalFromChunk,
  isTerminalPromptVisible,
  stripInactiveTerminalWorkingBanners,
} from "./terminalWorkingState";

describe("terminal working state", () => {
  it("detects a prompt when a terminal status line follows it", () => {
    expect(isTerminalPromptVisible("• Working (4s • esc to interrupt)\n> Use /skills to list available skills\ngpt-5.5 medium")).toBe(true);
  });

  it("prefers a visible Codex prompt over an older working banner", () => {
    const signal = getTerminalSignalFromChunk(
      "",
      "• Working (4s • esc to interrupt)\n> Use /skills to list available skills\ngpt-5.5 medium",
      "codex",
    );

    expect(signal.signalKind).toBe("idle_prompt");
  });

  it("does not treat the submitted Codex input line before the working banner as idle", () => {
    const signal = getTerminalSignalFromChunk(
      "",
      "› Run /review on my current changes\n• Working (4s • esc to interrupt)",
      "codex",
    );

    expect(signal.signalKind).toBe("working_output");
  });

  it("hides working banners from display when the actor is not working", () => {
    expect(stripInactiveTerminalWorkingBanners("\n◦ Working  9m 41s • esc to interrupt)", "idle")).toBe("");
  });

  it("hides corrupted working banners from display when the actor is not working", () => {
    expect(stripInactiveTerminalWorkingBanners("•�Working      40  03", "idle")).toBe("");
  });

  it("does not trim non-banner output while filtering", () => {
    expect(stripInactiveTerminalWorkingBanners("\nhello\n", "idle")).toBe("\nhello\n");
  });

  it("hides working banners from display while the actor is working", () => {
    expect(stripInactiveTerminalWorkingBanners("\n◦ Working  9m 41s • esc to interrupt)", "working")).toBe("");
  });

  it("hides working banners split across live terminal chunks", () => {
    const first = filterTerminalWorkingBannerChunk("", "• ");
    expect(first.visible).toBe("");
    expect(first.nextTail).toBe("• ");

    const second = filterTerminalWorkingBannerChunk(first.nextTail, "Working  (14h 17m 43s • esc to interrupt)\n> /");
    expect(second.visible).toBe("> /");
    expect(second.nextTail).toBe("");
  });
});
