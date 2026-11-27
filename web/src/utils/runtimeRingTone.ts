import type { Actor } from "../types";
import { hasManagedRuntimeOutput } from "./headlessRuntimeSupport";
import { UNOBSERVABLE_WORKING_STATE } from "./terminalWorkingState";

export type RuntimeRingTone = "stopped" | "idle" | "active" | "attention";

/** The member and, when one exists, the phase of its live reply. */
export interface RuntimeRingSubject {
  actor: Actor;
  liveWorkCard?: { phase: string } | null;
  /** Accepted so a dock item can be passed as it is; the rule reads the runner off the actor. */
  runner?: string;
}

/**
 * The one rule for a member's state colour. The member dock and the sidebar's bars both call
 * it, so a member cannot read differently in the two places.
 */
export function getRuntimeRingTone(
  item: RuntimeRingSubject,
  isRunning: boolean,
  workingState: string,
  // A terminal still waiting for its startup input past the shown delay.
  startupWaiting = false,
): RuntimeRingTone {
  if (item.actor.runtime_confirmation_required) return "attention";
  if (startupWaiting) return "attention";
  if (item.actor.runtime_confirmation_paused === true) return "attention";
  const actorState = String(workingState || "")
    .trim()
    .toLowerCase();
  // The daemon could not observe this agent: neither running nor stopped.
  if (actorState === UNOBSERVABLE_WORKING_STATE) return "attention";
  // Managed runtimes expose the current daemon-owned state. Task history can
  // retain an older outcome after recovery and must not override this ring.
  if (hasManagedRuntimeOutput(item.actor)) {
    if (!isRunning) return "stopped";
    if (actorState === "stuck" || actorState === "error") return "attention";
    if (actorState === "working") return "active";
    return "idle";
  }

  if (!isRunning) return item.liveWorkCard?.phase === "failed" ? "attention" : "stopped";

  if (actorState === "error" || item.liveWorkCard?.phase === "failed") return "attention";

  if (actorState === "working") return "active";

  if (item.liveWorkCard?.phase === "pending") return "active";
  if (item.liveWorkCard?.phase === "streaming") return "active";
  return "idle";
}
