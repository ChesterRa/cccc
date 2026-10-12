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
export const DELIVERY_STEPS = ["queued", "woken", "claimed", "completed"] as const;

/**
 * Where the latest delivery stands: how many steps finished and whether the next
 * one is underway or failed. Callback acceptance (delivered) is never completion.
 */
export function deliveryProgress(
  state: DotsActorState,
): { finished: number; next: "current" | "error" } | null {
  switch (state.phase) {
    case "pending":
    case "delivering":
      return { finished: 1, next: "current" };
    case "delivered":
      return { finished: 2, next: "current" };
    case "working":
      return { finished: 3, next: "current" };
    case "failed":
      return { finished: 1, next: "error" };
    case "interrupted":
      return { finished: 3, next: "error" };
    default:
      if (!state.last_execution) return null;
      return state.last_execution.status === "done"
        ? { finished: 4, next: "current" }
        : { finished: 3, next: "error" };
  }
}
