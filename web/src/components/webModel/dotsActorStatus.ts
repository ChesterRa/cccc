import type { DotsActorState } from "../../services/api";

export function phaseTone(phase: DotsActorState["phase"]) {
  if (phase === "ready" || phase === "working")
    return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  if (phase === "failed" || phase === "interrupted")
    return "bg-rose-500/10 text-rose-700 dark:text-rose-300";
  if (phase === "delivering" || phase === "delivered" || phase === "pending")
    return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
  return "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

export type StepState = "done" | "current" | "error" | "todo";
export const DELIVERY_STEPS = ["notify", "waiting", "working", "completed"] as const;

/**
 * The current delivery stage, not the next unfulfilled milestone. A completion
 * receipt may describe the last task while the Actor is idle or paused.
 */
export function deliveryProgress(
  state: DotsActorState,
): { finished: number; next: "current" | "error" } | null {
  switch (state.phase) {
    case "pending":
    case "delivering":
      return { finished: 0, next: "current" };
    case "delivered":
      return { finished: 1, next: "current" };
    case "working":
      return { finished: 2, next: "current" };
    case "failed":
      return { finished: 0, next: "error" };
    case "interrupted":
      return { finished: 2, next: "error" };
    default:
      return state.last_execution?.status === "done" ? { finished: 4, next: "current" } : null;
  }
}
