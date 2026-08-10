import { describe, expect, it } from "vite-plus/test";

import { getActorTabIndicatorState } from "../../src/components/tabBarIndicator";
import {
  QUIET_RUN_INDICATOR_DOT_CLASS,
  STOPPED_INDICATOR_DOT_CLASS,
} from "../../src/utils/statusIndicators";

describe("getActorTabIndicatorState", () => {
  it("only accents tabs that are actively working", () => {
    expect(getActorTabIndicatorState({ isRunning: true, workingState: "working" }).labelClass).toBe(
      "text-emerald-700 dark:text-emerald-300",
    );
    expect(getActorTabIndicatorState({ isRunning: true, workingState: "stuck" }).labelClass).toBe(
      "",
    );
  });

  it("does not override label color for non-accent states", () => {
    expect(getActorTabIndicatorState({ isRunning: true, workingState: "waiting" }).labelClass).toBe(
      "",
    );
    expect(getActorTabIndicatorState({ isRunning: true, workingState: "idle" }).labelClass).toBe(
      "",
    );
    expect(
      getActorTabIndicatorState({ isRunning: false, workingState: "working" }).labelClass,
    ).toBe("");
  });

  it("renders quiet running as the shared solid running tone", () => {
    expect(getActorTabIndicatorState({ isRunning: true, workingState: "" }).dotClass).toBe(
      QUIET_RUN_INDICATOR_DOT_CLASS,
    );
    expect(getActorTabIndicatorState({ isRunning: true, workingState: "" }).dotClass).toContain(
      "bg-emerald",
    );
  });

  it("keeps stopped when the actor itself is not running", () => {
    expect(getActorTabIndicatorState({ isRunning: false, workingState: "stopped" }).dotClass).toBe(
      STOPPED_INDICATOR_DOT_CLASS,
    );
  });

  it("can show quiet running during hydration when runtime state is still unknown", () => {
    expect(
      getActorTabIndicatorState({ isRunning: false, workingState: "", assumeRunning: true })
        .dotClass,
    ).toBe(QUIET_RUN_INDICATOR_DOT_CLASS);
  });
});
