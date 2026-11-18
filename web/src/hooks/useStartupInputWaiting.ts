import { useEffect, useReducer } from "react";
import type { Actor } from "../types";

/** A grace period before a terminal's startup wait is shown. Terminals observed so far reach
 *  their chat input 2 to 6 seconds after starting, and the wait is re-read every 3 seconds, so
 *  most ordinary starts clear first; a slow start or slow request can still show it briefly. */
export const STARTUP_INPUT_WAIT_SHOWN_AFTER_MS = 8000;

// When this page first saw each terminal process waiting, so remounting a view does not
// restart the grace period. Only the newest few are kept.
const firstSeenAt = new Map<string, number>();
const FIRST_SEEN_LIMIT = 64;

export function startupInputGeneration(actor?: Actor | null): string {
  if (!actor?.running) return "";
  return String(actor.runtime_startup_input?.generation || "").trim();
}

/** Whether this Actor's terminal has been waiting for its startup input past the grace period. */
export function useStartupInputWaiting(groupId: string, actor?: Actor | null): boolean {
  const generation = startupInputGeneration(actor);
  // The process is per member; members of different Groups can share a name and start time.
  const key = generation
    ? [String(groupId || ""), String(actor?.id || ""), generation].join("\u0000")
    : "";
  const [, rerender] = useReducer((count: number) => count + 1, 0);
  useEffect(() => {
    if (!key) return;
    if (!firstSeenAt.has(key)) {
      firstSeenAt.set(key, Date.now());
      while (firstSeenAt.size > FIRST_SEEN_LIMIT) {
        const oldest = firstSeenAt.keys().next().value;
        if (oldest === undefined) break;
        firstSeenAt.delete(oldest);
      }
    }
    const remaining = (firstSeenAt.get(key) ?? 0) + STARTUP_INPUT_WAIT_SHOWN_AFTER_MS - Date.now();
    if (remaining <= 0) return;
    const timer = window.setTimeout(rerender, remaining);
    return () => window.clearTimeout(timer);
  }, [key]);
  if (!key) return false;
  const seen = firstSeenAt.get(key);
  return seen !== undefined && Date.now() - seen >= STARTUP_INPUT_WAIT_SHOWN_AFTER_MS;
}
