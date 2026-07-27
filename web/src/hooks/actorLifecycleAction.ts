import type { Actor } from "../types";

export function resolveActorLifecycleRunning(
  actor: Pick<Actor, "running" | "enabled">,
  runningOverride?: boolean,
): boolean {
  return runningOverride ?? actor.running ?? actor.enabled ?? false;
}
