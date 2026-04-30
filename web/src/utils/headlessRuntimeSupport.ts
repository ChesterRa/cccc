const STANDARD_WEB_HEADLESS_RUNTIMES = new Set(["codex", "claude", "web_model"]);

export type ActorRunner = "pty" | "headless";

type RunnerSource = {
  runner?: unknown;
  runner_effective?: unknown;
} | null | undefined;

export function normalizeActorRunner(runner: unknown): ActorRunner {
  return String(runner || "").trim().toLowerCase() === "headless" ? "headless" : "pty";
}

export function getEffectiveActorRunner(actor: RunnerSource): ActorRunner {
  if (!actor) return "pty";
  return normalizeActorRunner(actor.runner_effective || actor.runner || "pty");
}

export function isHeadlessActorRunner(actor: RunnerSource): boolean {
  return getEffectiveActorRunner(actor) === "headless";
}

export function supportsStandardWebHeadlessRuntime(runtime: string | null | undefined): boolean {
  return STANDARD_WEB_HEADLESS_RUNTIMES.has(String(runtime || "").trim().toLowerCase());
}
