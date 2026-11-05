/** The worker asks an already-open workbench to show a notification target. */
export const PUSH_OPEN_MESSAGE = "cccc:push:open";

/** The dedicated push worker's script, under its own narrow scope. */
export const PUSH_WORKER_PATH = "/ui/push/sw.js";

/**
 * How long after the worker's own deadline the page still acts on a click
 * (the worker keeps the same limit, LATE_CLICK_MS in public/push/sw.js). A page
 * or browser that responds slowly after the click, or a frozen background tab,
 * can receive it tens of seconds late, and the worker never acts on a click
 * again once it has handed it to a page.
 */
export const LATE_CLICK_MS = 60_000;

export type PushMessageTarget = {
  groupId: string;
  eventId: string;
  deadline: number;
  /** When the worker began handling the click, if supplied by the worker. */
  startedAt?: number;
};

export function parsePushMessageTarget(value: unknown): PushMessageTarget | null {
  if (!value || typeof value !== "object") return null;
  const data = value as Record<string, unknown>;
  if (data.type !== PUSH_OPEN_MESSAGE || data.v !== 1) return null;
  if (
    typeof data.deadline_ms !== "number" ||
    !Number.isFinite(data.deadline_ms) ||
    data.deadline_ms + LATE_CLICK_MS < Date.now()
  )
    return null;
  const groupId = typeof data.group_id === "string" ? data.group_id.trim() : "";
  const eventId = typeof data.event_id === "string" ? data.event_id.trim() : "";
  if (
    !groupId ||
    !eventId ||
    groupId.length > 256 ||
    eventId.length > 256 ||
    Array.from(groupId + eventId).some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || code === 127;
    })
  )
    return null;
  const startedAt =
    typeof data.started_at_ms === "number" &&
    Number.isFinite(data.started_at_ms) &&
    data.started_at_ms > 0 &&
    data.started_at_ms <= data.deadline_ms
      ? data.started_at_ms
      : undefined;
  return { groupId, eventId, deadline: data.deadline_ms, ...(startedAt ? { startedAt } : {}) };
}

/** Whether `scriptURL` is the dedicated same-origin push worker. */
export function isPushWorkerScript(scriptURL: string): boolean {
  try {
    const url = new URL(scriptURL);
    return (
      url.origin === window.location.origin &&
      url.pathname === PUSH_WORKER_PATH &&
      !url.search &&
      !url.hash
    );
  } catch {
    return false;
  }
}

/** Only the dedicated same-origin worker may request this navigation. */
export function isPushWorker(source: MessageEventSource | null): boolean {
  return (
    !!source &&
    "scriptURL" in source &&
    typeof source.scriptURL === "string" &&
    isPushWorkerScript(source.scriptURL)
  );
}
