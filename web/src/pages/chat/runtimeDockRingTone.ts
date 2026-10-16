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
