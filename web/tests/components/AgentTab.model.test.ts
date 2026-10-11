import { describe, expect, it } from "vite-plus/test";

import {
  actorHasRuntimeResumeFailure,
  actorSupportsNewSession,
  shouldFetchStoppedTerminalTail,
} from "../../src/components/AgentTab.model";

describe("AgentTab stopped terminal tail model", () => {
  it("offers explicit new-session control for every managed resume runtime", () => {
    for (const runtime of ["claude", "codex", "grok", "pi", "opencode", "kilo"]) {
      expect(actorSupportsNewSession(runtime)).toBe(true);
    }
    expect(actorSupportsNewSession("cline")).toBe(false);
  });
  it.each(["kimi", "auggie", "kiro", "hermes", "droid"])(
    "only offers managed reset for %s in ACP mode",
    (runtime) => {
      expect(actorSupportsNewSession(runtime, "acp")).toBe(true);
      expect(actorSupportsNewSession(runtime, "default")).toBe(false);
    },
  );

  it("detects persisted runtime resume failures from actor state", () => {
    expect(actorHasRuntimeResumeFailure({ runtime_session_status: "resume_failed" })).toBe(true);
    expect(actorHasRuntimeResumeFailure({ runtime_session_status: "usable" })).toBe(false);
  });

  it("does not fetch stopped terminal output while the actor has an in-flight lifecycle action", () => {
    expect(
      shouldFetchStoppedTerminalTail({
        activated: true,
        isRunning: false,
        isHeadless: false,
        groupId: "g1",
        actorId: "Development",
        isActorBusy: true,
      }),
    ).toBe(false);
  });

  it("fetches stopped terminal output only for an activated non-headless stopped actor", () => {
    expect(
      shouldFetchStoppedTerminalTail({
        activated: true,
        isRunning: false,
        isHeadless: false,
        groupId: "g1",
        actorId: "Development",
        isActorBusy: false,
      }),
    ).toBe(true);
    expect(
      shouldFetchStoppedTerminalTail({
        activated: true,
        isRunning: true,
        isHeadless: false,
        groupId: "g1",
        actorId: "Development",
        isActorBusy: false,
      }),
    ).toBe(false);
  });
});
