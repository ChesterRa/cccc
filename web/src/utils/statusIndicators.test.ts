import { describe, expect, it } from "vite-plus/test";

import { getRuntimeIndicatorState } from "./statusIndicators";

describe("status indicators", () => {
  it("distinguishes a failed turn from a stopped process and clears it on new work", () => {
    const failed = getRuntimeIndicatorState({ isRunning: true, workingState: "error" });
    expect(failed.tone).toBe("error");
    expect(getRuntimeIndicatorState({ isRunning: true, workingState: "working" }).tone).toBe(
      "working",
    );
    expect(getRuntimeIndicatorState({ isRunning: true, workingState: "idle" }).tone).toBe("run");
    expect(getRuntimeIndicatorState({ isRunning: false, workingState: "error" }).tone).toBe("stop");
  });

  it("shows an agent the daemon could not observe as needing attention, never as stopped", () => {
    expect(getRuntimeIndicatorState({ isRunning: true, workingState: "unobservable" }).tone).toBe(
      "error",
    );
    expect(getRuntimeIndicatorState({ isRunning: false, workingState: "unobservable" }).tone).toBe(
      "error",
    );
    // The daemon's "unknown" turn state of a running agent is ordinary running.
    expect(getRuntimeIndicatorState({ isRunning: true, workingState: "unknown" }).tone).toBe("run");
  });
});
