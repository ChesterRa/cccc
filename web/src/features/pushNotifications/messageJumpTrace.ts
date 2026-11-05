/** Content-free timing for a message link or notification jump. */
const SAMPLE_MS = 1500;

type RequestState = "not_started" | "pending" | "resolved" | "failed";
type TargetState =
  | "in_viewport"
  | "outside_viewport"
  | "not_rendered"
  | "other_group"
  | "page_hidden";
type Trace = {
  groupId: string;
  eventId: string;
  source: "worker" | "url" | "retry";
  started: number;
  workerToPageMs: number | null;
  navigationAt: number | null;
  requestAt: number | null;
  requestMs: number | null;
  request: RequestState;
  httpStatus: number | null;
  errorKind: string | null;
  timer: number | null;
  sampled: boolean;
  initialVisible: boolean;
  lateMs: number;
  isCurrentGroup: () => boolean;
  isWaitingForUser: () => boolean;
};

let active: Trace | null = null;

function elapsed(start: number, end: number): number {
  return Math.max(0, Math.round(end - start));
}

function clear(trace: Trace): void {
  if (trace.timer !== null) window.clearTimeout(trace.timer);
  if (active === trace) active = null;
}

function targetState(trace: Trace): TargetState {
  if (document.hidden) return "page_hidden";
  if (!trace.isCurrentGroup()) return "other_group";
  const row = Array.from(document.querySelectorAll<HTMLElement>('[data-message-row="true"]')).find(
    (element) => element.dataset.messageId === trace.eventId,
  );
  const log = row?.closest<HTMLElement>('[role="log"]');
  if (!row || !log) return "not_rendered";
  const item = row.getBoundingClientRect();
  const area = log.getBoundingClientRect();
  return Math.min(item.right, area.right) > Math.max(item.left, area.left) &&
    Math.min(item.bottom, area.bottom) > Math.max(item.top, area.top)
    ? "in_viewport"
    : "outside_viewport";
}

function sample(trace: Trace, expectedAt: number, phase: "initial" | "after_response"): void {
  if (active !== trace) return;
  if (trace.timer !== null) window.clearTimeout(trace.timer);
  trace.timer = null;
  try {
    const now = performance.now();
    const target = targetState(trace);
    const late = elapsed(expectedAt, now);
    trace.lateMs = Math.max(trace.lateMs, late);
    if (phase === "initial") trace.initialVisible = target === "in_viewport";
    const noteworthy =
      trace.request === "failed" ||
      target !== "in_viewport" ||
      (trace.workerToPageMs ?? 0) >= SAMPLE_MS ||
      trace.lateMs >= 250 ||
      (!trace.initialVisible &&
        ((trace.requestMs ?? 0) >= SAMPLE_MS || phase === "after_response"));
    if (noteworthy) {
      // Do not log IDs, URLs, server messages or message contents.
      console.warn(
        "[CCCC message jump]",
        JSON.stringify({
          phase,
          source: trace.source,
          worker_to_page_ms: trace.workerToPageMs,
          page_to_open_ms:
            trace.navigationAt === null ? null : elapsed(trace.started, trace.navigationAt),
          request_ms: trace.requestMs,
          request: trace.request,
          http_status: trace.httpStatus,
          error_kind: trace.errorKind,
          target_at_sample: target,
          waiting_confirmation: trace.isWaitingForUser(),
          sample_ms: elapsed(trace.started, now),
          late_ms: trace.lateMs,
        }),
      );
    }
  } catch {
    // Diagnostics must never interfere with navigation.
  }
  trace.sampled = true;
  if (phase === "after_response" || trace.request === "resolved" || trace.request === "failed")
    clear(trace);
}

function schedule(trace: Trace, delay: number, phase: "initial" | "after_response"): void {
  if (trace.timer !== null) window.clearTimeout(trace.timer);
  const expectedAt = performance.now() + delay;
  trace.timer = window.setTimeout(() => sample(trace, expectedAt, phase), delay);
}

export function startMessageJumpTrace(
  groupId: string,
  eventId: string,
  source: Trace["source"],
  options: {
    workerToPageMs?: number | null;
    isCurrentGroup: () => boolean;
    isWaitingForUser: () => boolean;
  },
): void {
  if (active) clear(active);
  const trace: Trace = {
    groupId,
    eventId,
    source,
    started: performance.now(),
    workerToPageMs: options.workerToPageMs ?? null,
    navigationAt: null,
    requestAt: null,
    requestMs: null,
    request: "not_started",
    httpStatus: null,
    errorKind: null,
    timer: null,
    sampled: false,
    initialVisible: false,
    lateMs: 0,
    isCurrentGroup: options.isCurrentGroup,
    isWaitingForUser: options.isWaitingForUser,
  };
  active = trace;
  schedule(trace, SAMPLE_MS, "initial");
}

export function beginMessageJumpNavigation(groupId: string, eventId: string): void {
  const trace = active;
  if (!trace) return;
  if (trace.groupId !== groupId || trace.eventId !== eventId) clear(trace);
  else trace.navigationAt = performance.now();
}

export function cancelMessageJumpTrace(): void {
  if (active) clear(active);
}

/** Measure the open operation; its resolution does not prove the target is visible. */
export function startMessageWindowRequest(groupId: string, eventId: string) {
  const trace = active;
  if (!trace || trace.groupId !== groupId || trace.eventId !== eventId) return null;
  trace.requestAt = performance.now();
  trace.request = "pending";
  return (result: "resolved" | "failed") => {
    if (active !== trace || trace.requestAt === null) return;
    trace.request = result;
    trace.requestMs = elapsed(trace.requestAt, performance.now());
    if (trace.sampled) schedule(trace, 300, "after_response");
    else if (result === "failed") sample(trace, performance.now(), "after_response");
  };
}

/** Keep only a numeric status and a safe category; the server's error text is not logged. */
export function recordMessageJumpFailure(
  groupId: string,
  eventId: string,
  status?: number,
  code?: string,
): void {
  const trace = active;
  if (!trace || trace.groupId !== groupId || trace.eventId !== eventId) return;
  trace.httpStatus =
    typeof status === "number" && Number.isInteger(status) && status >= 0 && status <= 599
      ? status
      : null;
  trace.errorKind =
    code === "NETWORK_ERROR" ? "network" : trace.httpStatus !== null ? "http" : "other";
}
