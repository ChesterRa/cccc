export type RuntimeIndicatorTone = "stop" | "run" | "working" | "error";

export function getRuntimeIndicatorState(input: { isRunning: boolean; workingState: string }): {
  tone: RuntimeIndicatorTone;
} {
  if (!input.isRunning) return { tone: "stop" };
  const workingState = String(input.workingState || "")
    .trim()
    .toLowerCase();
  if (workingState === "working") return { tone: "working" };
  if (workingState === "error") return { tone: "error" };
  return { tone: "run" };
}
