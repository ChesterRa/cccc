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
});
