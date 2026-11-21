import { UNOBSERVABLE_WORKING_STATE } from "./terminalWorkingState";

export type RuntimeIndicatorTone = "stop" | "run" | "working" | "error";

export function getRuntimeIndicatorState(input: { isRunning: boolean; workingState: string }): {
  tone: RuntimeIndicatorTone;
} {
  const workingState = String(input.workingState || "")
    .trim()
    .toLowerCase();
  // Not observable is neither running nor stopped; it needs attention either way.
  if (workingState === UNOBSERVABLE_WORKING_STATE) return { tone: "error" };
  if (!input.isRunning) return { tone: "stop" };
  if (workingState === "working") return { tone: "working" };
  if (workingState === "error") return { tone: "error" };
  return { tone: "run" };
}
