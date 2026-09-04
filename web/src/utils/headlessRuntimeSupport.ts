export type ActorRunner = "pty" | "headless";

type RunnerSource =
  | { runner?: unknown; runner_effective?: unknown; runtime_state_source?: unknown }
  | null
  | undefined;

export function normalizeActorRunner(runner: unknown): ActorRunner {
  return String(runner || "")
    .trim()
    .toLowerCase() === "headless"
    ? "headless"
    : "pty";
}

export function getEffectiveActorRunner(actor: RunnerSource): ActorRunner {
  if (!actor) return "pty";
  return normalizeActorRunner(actor.runner_effective || actor.runner || "pty");
}

export function isHeadlessActorRunner(actor: RunnerSource): boolean {
  return getEffectiveActorRunner(actor) === "headless";
}

export function hasManagedRuntimeOutput(actor: RunnerSource): boolean {
  if (!actor) return false;
  const source = String(actor.runtime_state_source || "")
    .trim()
    .toLowerCase();
  return isHeadlessActorRunner(actor) || source === "managed_session" || source === "app_server";
}
