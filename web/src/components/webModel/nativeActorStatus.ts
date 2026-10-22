export function phaseTone(phase: string) {
  if (phase === "ready" || phase === "working")
    return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  if (phase === "failed" || phase === "interrupted")
    return "bg-rose-500/10 text-rose-700 dark:text-rose-300";
  if (["pending", "delivering", "delivered", "unclaimed"].includes(phase))
    return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
  return "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

export type StepState = "done" | "current" | "error" | "todo";
export const DELIVERY_STEPS = ["notify", "waiting", "working", "completed"] as const;

/** Current daemon-projected delivery only; past receipts belong in the summary. */
export function deliveryProgress(
  phase: string,
): { finished: number; next: "current" | "error" } | null {
  switch (phase) {
    case "pending":
    case "delivering":
      return { finished: 0, next: "current" };
    case "delivered":
      return { finished: 1, next: "current" };
    case "unclaimed":
      return { finished: 1, next: "error" };
    case "working":
      return { finished: 2, next: "current" };
    case "failed":
      return { finished: 0, next: "error" };
    case "interrupted":
      return { finished: 2, next: "error" };
    default:
      return null;
  }
}
