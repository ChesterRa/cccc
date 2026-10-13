import { describe, expect, it } from "vite-plus/test";

import { GROUP_STATUS_DOT_BASE_CLASS } from "./groupStatus";
import { getGroupPresenceDotClass, getRuntimeIndicatorState } from "./statusIndicators";

describe("status indicators", () => {
  it("keeps group dots at a fixed size across text scales", () => {
    expect(GROUP_STATUS_DOT_BASE_CLASS).toContain("size-[8px]");
  });

  it.each(["run", "paused", "idle", "stop"] as const)(
    "renders the %s group state as a quiet solid dot",
    (tone) => {
      const className = getGroupPresenceDotClass(tone);

      expect(className).toContain("bg-");
      expect(className).not.toContain("bg-transparent");
      expect(className).not.toContain("ring-");
      expect(className).not.toContain("shadow-");
    },
  );

  it("keeps the working runtime state visually emphasized", () => {
    const state = getRuntimeIndicatorState({ isRunning: true, workingState: "working" });

    expect(state.strongPulse).toBe(true);
    expect(state.dotClass).toContain("shadow-");
  });

  it("distinguishes a failed turn from a stopped process and clears it on new work", () => {
    const failed = getRuntimeIndicatorState({ isRunning: true, workingState: "error" });
    expect(failed.tone).toBe("error");
    expect(failed.pulse).toBe(false);
    expect(getRuntimeIndicatorState({ isRunning: true, workingState: "working" }).tone).toBe(
      "working",
    );
    expect(getRuntimeIndicatorState({ isRunning: true, workingState: "idle" }).tone).toBe("run");
    expect(getRuntimeIndicatorState({ isRunning: false, workingState: "error" }).tone).toBe("stop");
  });
});
