import type { WebModelBrowserSession, WebModelPairing } from "../../services/api";

/** Pairing is the routing authority; browser readiness alone never proves a binding. */
export function webModelConnectionState(
  pairing?: WebModelPairing,
  session?: WebModelBrowserSession | null,
) {
  const state = pairing?.state || "loading";
  const bound = Boolean(pairing?.url);
  const connecting = state === "waiting" || state === "awaiting_confirmation";
  const failed = ["failed", "interrupted", "cancelled"].includes(state);
  let displayState = bound && failed ? `replacement_${state}` : state;
  if (state === "bound") {
    if (session?.verification_required) displayState = "verification_required";
    else if (session?.login_required) displayState = "login_required";
    else if (pairing?.actor_enabled === false) displayState = "bound_stopped";
    else if (session?.active === false) displayState = "browser_closed";
  }
  return { state, displayState, bound, connecting, failed };
}
