import type { RuntimeDockItem } from "./runtimeDockItems";

export type RuntimeRingTone = "stopped" | "idle" | "active" | "attention";

export function getRuntimeRingTone(
  item: Pick<RuntimeDockItem, "actor" | "liveWorkCard" | "runner">,
  isRunning: boolean,
  workingState: string,
): RuntimeRingTone {
  const actorState = String(workingState || "").trim().toLowerCase();
  const stateSource = String(item.actor?.runtime_state_source || "").trim().toLowerCase();
  if (item.liveWorkCard?.phase === "failed") return "attention";
  if (!isRunning) return "stopped";

  // Headless runtimes already expose their real daemon-derived working state.
  // Live-work transcript state can lag during reconnect/catch-up, so don't let
  // stale pending/streaming cards light the ring after the actor is idle.
  if (item.runner === "headless") {
    if (actorState === "stuck") return "attention";
    if (actorState === "working") return "active";
    return "idle";
  }

  if (stateSource === "app_server") {
    if (actorState === "stuck") return "attention";
    if (actorState === "working") return "active";
    return "idle";
  }

  if (actorState === "working") return "active";

  if (item.liveWorkCard?.phase === "pending") return "active";
  if (item.liveWorkCard?.phase === "streaming") return "active";
  return "idle";
}
