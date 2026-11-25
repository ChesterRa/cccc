import { describe, expect, it } from "vite-plus/test";

import {
  filterTerminalWorkingBannerChunk,
  getActorDisplayWorkingState,
  getTerminalSignalFromChunk,
  isTerminalPromptVisible,
  stripInactiveTerminalWorkingBanners,
} from "./terminalWorkingState";

describe("terminal working state", () => {
  it("keeps an agent the daemon could not observe unobservable, whatever its terminal showed", () => {
    const actor = {
      enabled: true,
      runner: "pty",
      effective_working_state: "unknown",
      effective_working_reason: "runtime_owner_unavailable",
    };
    const now = Date.now();
    expect(getActorDisplayWorkingState(actor, { kind: "idle_prompt", updatedAt: now }, now)).toBe(
      "unobservable",
    );
    expect(
      getActorDisplayWorkingState(actor, { kind: "working_output", updatedAt: now }, now),
    ).toBe("unobservable");
    // The daemon's "unknown" turn state of a running agent stays as it was.
    const running = {
      running: true,
      runner: "headless",
      effective_working_state: "unknown",
      effective_working_reason: "headless_running_state_unknown",
    };
    expect(getActorDisplayWorkingState(running, null, now)).toBe("unknown");
    expect(
      getActorDisplayWorkingState(
        { running: true, runner: "pty", effective_working_state: "waiting" },
        { kind: "working_output", updatedAt: now },
        now,
      ),
    ).toBe("working");
    // Authoritative Antigravity lifecycle hooks are not overridden by PTY heuristics.
    expect(
      getActorDisplayWorkingState(
        {
          running: true,
          runner: "pty",
          runtime_state_source: "terminal",
          effective_working_state: "idle",
          effective_working_reason: "antigravity_hook",
        },
        { kind: "working_output", updatedAt: now },
        now,
      ),
    ).toBe("idle");
    expect(
      getActorDisplayWorkingState(
        {
          running: true,
          runner: "pty",
          runtime_state_source: "terminal",
          effective_working_state: "working",
          effective_working_reason: "antigravity_hook",
        },
        { kind: "idle_prompt", updatedAt: now },
        now,
      ),
    ).toBe("working");
  });

  it("detects a prompt when a terminal status line follows it", () => {
    expect(
      isTerminalPromptVisible(
        "• Working (4s • esc to interrupt)\n> Use /skills to list available skills\ngpt-5.5 medium",
      ),
    ).toBe(true);
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
    expect(
      stripInactiveTerminalWorkingBanners("\n◦ Working  9m 41s • esc to interrupt)", "idle"),
    ).toBe("");
  });

  it("hides corrupted working banners from display when the actor is not working", () => {
    expect(stripInactiveTerminalWorkingBanners("•�Working      40  03", "idle")).toBe("");
  });

  it("does not trim non-banner output while filtering", () => {
    expect(stripInactiveTerminalWorkingBanners("\nhello\n", "idle")).toBe("\nhello\n");
  });

  it("hides working banners from display while the actor is working", () => {
    expect(
      stripInactiveTerminalWorkingBanners("\n◦ Working  9m 41s • esc to interrupt)", "working"),
    ).toBe("");
  });

  it("hides working banners split across live terminal chunks", () => {
    const first = filterTerminalWorkingBannerChunk("", "• ");
    expect(first.visible).toBe("");
    expect(first.nextTail).toBe("• ");

    const second = filterTerminalWorkingBannerChunk(
      first.nextTail,
      "Working  (14h 17m 43s • esc to interrupt)\n> /",
    );
    expect(second.visible).toBe("> /");
    expect(second.nextTail).toBe("");
  });
});
