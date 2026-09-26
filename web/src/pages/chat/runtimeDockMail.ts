import type { LedgerEvent } from "../../types";

export type RuntimeDockMailInfo = { count: number; oldestTsMs: number | null };

/**
 * Outstanding mail per actor from the per-event `_read_status` map that the
 * backend attaches (with_read_status) and live `mail.read` events update
 * through the cursor boundary — same semantics as inbox::list_unread.
 * Scoped to the loaded event window.
 */
export function buildRuntimeDockMailInfo(events: LedgerEvent[]): Map<string, RuntimeDockMailInfo> {
  const info = new Map<string, RuntimeDockMailInfo>();
  if (!Array.isArray(events)) return info;
  for (const ev of events) {
    if (ev?.kind !== "chat.message") continue;
    if ((ev.data as { message_mode?: unknown } | undefined)?.message_mode !== "mail") {
      continue;
    }
    const readStatus = ev._read_status;
    if (!readStatus || typeof readStatus !== "object") continue;
    const tsMs = Date.parse(String(ev.ts || ""));
    if (!Number.isFinite(tsMs)) continue;
    for (const [actorId, read] of Object.entries(readStatus)) {
      if (read !== false) continue;
      const entry = info.get(actorId);
      if (entry) {
        entry.count += 1;
        if (entry.oldestTsMs === null || tsMs < entry.oldestTsMs) {
          entry.oldestTsMs = tsMs;
        }
      } else {
        info.set(actorId, { count: 1, oldestTsMs: tsMs });
      }
    }
  }
  return info;
}

const ACTOR_LIFECYCLE_KINDS = new Set([
  "actor.start",
  "actor.stop",
  "actor.restart",
  "actor.new_session",
]);

/**
 * Latest lifecycle outcome per actor: `number` ts when its most recent
 * lifecycle event is actor.stop, null when the most recent is a start/restart.
 * Actors absent from the map have no lifecycle event in the given window.
 */
export function buildActorStoppedSinceMap(events: LedgerEvent[]): Map<string, number | null> {
  const outcome = new Map<string, number | null>();
  if (!Array.isArray(events)) return outcome;
  for (let i = events.length - 1; i >= 0; i -= 1) {
    const ev = events[i];
    const kind = String(ev?.kind || "");
    if (!ACTOR_LIFECYCLE_KINDS.has(kind)) continue;
    const actorId = String((ev?.data as { actor_id?: unknown } | undefined)?.actor_id || "").trim();
    if (!actorId || outcome.has(actorId)) continue;
    const tsMs = kind === "actor.stop" ? Date.parse(String(ev?.ts || "")) : NaN;
    outcome.set(actorId, Number.isFinite(tsMs) ? tsMs : null);
  }
  return outcome;
}
