import { describe, expect, it } from "vite-plus/test";

import {
  getTerminalSignalRefreshDelayMs,
  resolveActorRunningState,
} from "../../src/hooks/useActorDisplayState";

describe("useActorDisplayState terminal signal refresh scheduling", () => {
  it("schedules one refresh just after an idle prompt signal expires", () => {
    expect(getTerminalSignalRefreshDelayMs({ kind: "idle_prompt", updatedAt: 1000 }, 1000)).toBe(
      3050,
    );
    expect(getTerminalSignalRefreshDelayMs({ kind: "idle_prompt", updatedAt: 1000 }, 4050)).toBe(0);
  });

  it("schedules one refresh just after a working output signal expires", () => {
    expect(getTerminalSignalRefreshDelayMs({ kind: "working_output", updatedAt: 1000 }, 1000)).toBe(
      5050,
    );
    expect(getTerminalSignalRefreshDelayMs({ kind: "working_output", updatedAt: 1000 }, 6050)).toBe(
      0,
    );
  });

  it("does not schedule refresh work when no signal exists", () => {
    expect(getTerminalSignalRefreshDelayMs(null, 1000)).toBeNull();
  });
});

describe("resolveActorRunningState", () => {
  it("keeps a cached stopped actor provisionally running until status refresh", () => {
    expect(
      resolveActorRunningState({
        actor: { running: false, enabled: true },
        actorStatusProvisional: true,
      }),
    ).toEqual({ isRunning: true, assumeRunning: true });
  });

  it("accepts a confirmed stopped actor after status refresh", () => {
    expect(
      resolveActorRunningState({
        actor: { running: false, enabled: true },
        actorStatusProvisional: false,
      }),
    ).toEqual({ isRunning: false, assumeRunning: false });
  });
});
