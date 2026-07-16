import type { HeadlessStreamEvent } from "../types";

export function replayHeadlessSnapshotEvents(
  events: HeadlessStreamEvent[],
  onEvent: (event: HeadlessStreamEvent) => void,
): void {
  for (const event of Array.isArray(events) ? events : []) {
    if (!event || typeof event !== "object") continue;
    const actorId = String(event.actor_id || "").trim();
    const eventType = String(event.type || "").trim();
    if (!actorId || !eventType) continue;
    onEvent(event);
  }
}
