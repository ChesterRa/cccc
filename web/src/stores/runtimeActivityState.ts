import type { RuntimeActivityEvent } from "../types";

export type RuntimeActivityByActor = Record<string, RuntimeActivityEvent[]>;
export type RuntimeActivityByGroup = Record<string, RuntimeActivityByActor>;

const COMPLETED_RETENTION_MS = 8_000;
const ACTIVE_RETENTION_MS = 5 * 60_000;
const ACTIVITY_LIMIT_PER_ACTOR = 24;

function timestampMs(event: RuntimeActivityEvent): number {
  const parsed = Date.parse(String(event.ts || ""));
  return Number.isFinite(parsed) ? parsed : 0;
}

function isActive(event: RuntimeActivityEvent): boolean {
  return event.status === "started" || event.status === "waiting" || event.status === "stuck";
}

function isTerminal(event: RuntimeActivityEvent): boolean {
  return event.status === "completed" || event.status === "failed";
}

function shouldReplaceActivity(
  existing: RuntimeActivityEvent,
  incoming: RuntimeActivityEvent,
): boolean {
  if (isTerminal(incoming) && existing.status === "stuck") return true;
  if (incoming.status === "stuck" && isTerminal(existing)) return false;
  return timestampMs(incoming) >= timestampMs(existing);
}

export function mergeRuntimeActivityEvents(
  previous: RuntimeActivityEvent[],
  incoming: RuntimeActivityEvent[],
): RuntimeActivityEvent[] {
  const byActivityId = new Map<string, RuntimeActivityEvent>();
  for (const event of [...previous, ...incoming]) {
    const activityId = String(event?.activity_id || "").trim();
    const eventId = String(event?.id || "").trim();
    if (!activityId || !eventId) continue;
    const existing = byActivityId.get(activityId);
    if (!existing || shouldReplaceActivity(existing, event)) {
      byActivityId.set(activityId, event);
    }
  }
  return Array.from(byActivityId.values())
    .sort(
      (left, right) => timestampMs(left) - timestampMs(right) || left.id.localeCompare(right.id),
    )
    .slice(-ACTIVITY_LIMIT_PER_ACTOR);
}

export function ingestRuntimeActivityEvents(
  state: RuntimeActivityByGroup,
  groupId: string,
  incoming: RuntimeActivityEvent[],
): RuntimeActivityByGroup {
  const gid = String(groupId || "").trim();
  if (!gid || incoming.length <= 0) return state;
  const previousGroup = state[gid] || {};
  const nextGroup = { ...previousGroup };
  let changed = false;
  for (const event of incoming) {
    const actorId = String(event?.actor_id || "").trim();
    if (!actorId || String(event.group_id || "").trim() !== gid) continue;
    const previous = previousGroup[actorId] || [];
    const next = mergeRuntimeActivityEvents(previous, [event]);
    if (JSON.stringify(previous) === JSON.stringify(next)) continue;
    nextGroup[actorId] = next;
    changed = true;
  }
  return changed ? { ...state, [gid]: nextGroup } : state;
}

export function pruneRuntimeActivityEvents(
  state: RuntimeActivityByGroup,
  nowMs: number,
): RuntimeActivityByGroup {
  let changed = false;
  const nextState: RuntimeActivityByGroup = {};
  for (const [groupId, actors] of Object.entries(state)) {
    const nextActors: RuntimeActivityByActor = {};
    for (const [actorId, events] of Object.entries(actors)) {
      const nextEvents = events.filter((event) => {
        const age = Math.max(0, nowMs - timestampMs(event));
        return age <= (isActive(event) ? ACTIVE_RETENTION_MS : COMPLETED_RETENTION_MS);
      });
      if (nextEvents.length > 0) nextActors[actorId] = nextEvents;
      if (nextEvents.length !== events.length) changed = true;
    }
    if (Object.keys(nextActors).length > 0) nextState[groupId] = nextActors;
    if (Object.keys(nextActors).length !== Object.keys(actors).length) changed = true;
  }
  return changed ? nextState : state;
}
