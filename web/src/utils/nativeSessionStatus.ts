import type { Actor } from "../types";

export const NATIVE_SESSION_AVAILABLE = "nativeSession.available";

/**
 * Status label key, in the actors namespace unless qualified, for an Actor
 * bound to a native session. CCCC does not run that session, so `running`
 * only says whether CCCC can hand it messages; it never means the session or
 * its model stopped. Every surface that shows the state uses these keys.
 */
export function nativeSessionStatusKey(
  actor: Pick<Actor, "enabled" | "running">,
  groupState: string | undefined,
): string {
  if (actor.enabled === false) return "webAccessPaused";
  if (groupState === "paused") return "nativeSession.groupPaused";
  if (groupState === "stopped") return "nativeSession.groupStopped";
  if (actor.running === true) return NATIVE_SESSION_AVAILABLE;
  if (
    actor.enabled === true &&
    actor.running === false &&
    (groupState === "active" || groupState === "idle")
  )
    return "nativeSession.unavailable";
  return "common:loading";
}
