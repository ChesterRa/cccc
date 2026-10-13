import type { RuntimeDockItem } from "./runtimeDockItems";
import { hasManagedRuntimeOutput } from "../../utils/headlessRuntimeSupport";

export type RuntimeRingTone = "stopped" | "idle" | "active" | "attention";

export function getRuntimeRingTone(
  item: Pick<RuntimeDockItem, "actor" | "liveWorkCard" | "runner">,
  isRunning: boolean,
  workingState: string,
): RuntimeRingTone {
  const actorState = String(workingState || "")
    .trim()
    .toLowerCase();
  if (!isRunning) return item.liveWorkCard?.phase === "failed" ? "attention" : "stopped";

  // Headless runtimes already expose their real daemon-derived working state.
  // Live-work transcript state can lag during reconnect/catch-up, so don't let
  // stale pending/streaming cards light the ring after the actor is idle.
  if (hasManagedRuntimeOutput(item.actor)) {
    if (actorState === "stuck" || actorState === "error") return "attention";
    if (actorState === "working") return "active";
    if (item.liveWorkCard?.phase === "failed") return "attention";
    return "idle";
  }

  if (actorState === "error" || item.liveWorkCard?.phase === "failed") return "attention";

  if (actorState === "working") return "active";

  if (item.liveWorkCard?.phase === "pending") return "active";
  if (item.liveWorkCard?.phase === "streaming") return "active";
  return "idle";
}
