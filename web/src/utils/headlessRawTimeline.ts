import type { HeadlessStreamEvent } from "../types";

export type HeadlessRawTraceEntry =
  | {
      kind: "message";
      id: string;
      order: number;
      ts: string;
      streamId: string;
      streamPhase: string;
      text: string;
      completed: boolean;
      live: boolean;
    }
  | {
      kind: "event";
      id: string;
      order: number;
      ts: string;
      eventType: string;
      badge: string;
      title: string;
      detailLines: string[];
      tone: "neutral" | "info" | "success" | "warning" | "error";
      live: boolean;
    };

function normalizeText(value: unknown): string {
  return String(value || "").trim();
}

function normalizeArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.map((item) => normalizeText(item)).filter(Boolean);
}

function eventTimestamp(event: HeadlessStreamEvent): string {
  return normalizeText(event.ts);
}

function compareEvents(left: HeadlessStreamEvent, right: HeadlessStreamEvent): number {
  const leftTs = eventTimestamp(left);
  const rightTs = eventTimestamp(right);
  if (leftTs && rightTs && leftTs !== rightTs) return leftTs.localeCompare(rightTs);
  return 0;
}

function extractErrorText(data: Record<string, unknown>): string {
  const errorRecord = data.error && typeof data.error === "object" ? data.error as Record<string, unknown> : null;
  if (errorRecord) {
    return [
      normalizeText(errorRecord.message),
      normalizeText(errorRecord.detail),
      normalizeText(errorRecord.code),
    ].filter(Boolean).join(" | ");
  }
  const direct = normalizeText(data.error);
  if (direct) return direct;
  return "";
}

function mapActivityBadge(rawItemType: string, kind: string): string {
  const raw = normalizeText(rawItemType).toLowerCase();
  const normalizedKind = normalizeText(kind).toLowerCase();
  if (raw === "reasoning") return "THINK";
  if (raw === "plan") return "PLAN";
  if (raw === "commandexecution") return "RUN";
  if (raw === "filechange") return "PATCH";
  if (raw === "mcptoolcall" || normalizedKind === "tool") return "TOOL";
  if (raw.startsWith("hook_")) return "HOOK";
  if (raw.startsWith("task_")) return "TASK";
  if (normalizedKind === "queued") return "QUEUE";
  return (normalizedKind || raw || "EVENT").replace(/[^a-z0-9]+/gi, "_").toUpperCase();
}

function buildEventEntry(event: HeadlessStreamEvent, order: number): HeadlessRawTraceEntry | null {
  const eventType = normalizeText(event.type);
  const data = event.data && typeof event.data === "object" ? event.data as Record<string, unknown> : {};
  if (!eventType || eventType.startsWith("headless.message.")) return null;

  const detailLines: string[] = [];
  let badge = "EVENT";
  let title = eventType.replace(/^headless\./, "").replace(/\./g, " ");
  let tone: Extract<HeadlessRawTraceEntry, { kind: "event" }>["tone"] = "neutral";
  let live = false;

  if (eventType.startsWith("headless.activity.")) {
    const activityId = normalizeText(data.activity_id);
    badge = mapActivityBadge(normalizeText(data.raw_item_type), normalizeText(data.kind));
    const command = normalizeText(data.command);
    const summary = normalizeText(data.summary);
    const detail = normalizeText(data.detail);
    title = command || summary || detail || title;
    detailLines.push(...[
      summary && summary !== title ? summary : "",
      detail,
      command && command !== title ? `command: ${command}` : "",
      normalizeText(data.cwd) ? `cwd: ${normalizeText(data.cwd)}` : "",
      normalizeText(data.query) ? `query: ${normalizeText(data.query)}` : "",
      normalizeText(data.tool_name) ? `tool: ${normalizeText(data.tool_name)}` : "",
      normalizeText(data.server_name) ? `server: ${normalizeText(data.server_name)}` : "",
      normalizeArray(data.file_paths).length > 0 ? `files: ${normalizeArray(data.file_paths).join(", ")}` : "",
    ].filter(Boolean));
    const status = normalizeText(data.status).toLowerCase() || eventType.replace("headless.activity.", "");
    live = status !== "completed";
    tone = status === "completed" ? "success" : "info";
  } else if (eventType.startsWith("headless.turn.") || eventType.startsWith("headless.control.")) {
    const isFailed = eventType.endsWith(".failed");
    const isQueued = eventType.endsWith(".queued") || eventType.endsWith(".requeued");
    const isStarted = eventType.endsWith(".started") || eventType.endsWith(".progress");
    badge = eventType.startsWith("headless.control.") ? "CONTROL" : "TURN";
    title = normalizeText(data.status) || eventType.replace(/^headless\./, "").replace(/\./g, " ");
    const errorText = extractErrorText(data);
    if (normalizeText(data.control_kind)) detailLines.push(`kind: ${normalizeText(data.control_kind)}`);
    if (normalizeText(data.turn_id)) detailLines.push(`turn: ${normalizeText(data.turn_id)}`);
    if (errorText) detailLines.push(errorText);
    tone = isFailed ? "error" : isQueued ? "warning" : isStarted ? "info" : "success";
    live = !eventType.endsWith(".completed") && !eventType.endsWith(".failed");
  } else if (eventType === "headless.thread.started" || eventType === "headless.session.stopped") {
    badge = "STATE";
    title = eventType === "headless.thread.started" ? "Thread started" : "Session stopped";
    if (normalizeText(data.thread_id)) detailLines.push(`thread: ${normalizeText(data.thread_id)}`);
    tone = eventType === "headless.session.stopped" ? "warning" : "info";
  } else if (eventType === "headless.item.started") {
    badge = "ITEM";
    const item = data.item && typeof data.item === "object" ? data.item as Record<string, unknown> : {};
    title = normalizeText(item.type) || "item started";
    if (normalizeText(item.id)) detailLines.push(`id: ${normalizeText(item.id)}`);
    tone = "info";
    live = true;
  } else {
    const errorText = extractErrorText(data);
    if (errorText) detailLines.push(errorText);
    tone = errorText ? "error" : "neutral";
  }

  return {
    kind: "event",
    id: normalizeText(data.activity_id)
      ? `activity:${normalizeText(data.activity_id)}`
      : eventType.startsWith("headless.turn.") || eventType.startsWith("headless.control.")
        ? `${eventType.startsWith("headless.control.") ? "control" : "turn"}:${normalizeText(data.turn_id) || normalizeText(data.event_id) || normalizeText(event.id) || order}`
        : normalizeText(event.id) || `${normalizeText(event.actor_id)}:${eventType}:${order}`,
    order,
    ts: eventTimestamp(event),
    eventType,
    badge,
    title,
    detailLines,
    tone,
    live,
  };
}

export function buildHeadlessRawTraceEntries(events: HeadlessStreamEvent[]): HeadlessRawTraceEntry[] {
  const sortedEvents = (Array.isArray(events) ? events.slice() : []).sort(compareEvents);
  const entries: HeadlessRawTraceEntry[] = [];
  const messageEntryByStreamId = new Map<string, Extract<HeadlessRawTraceEntry, { kind: "message" }>>();
  const eventEntryById = new Map<string, Extract<HeadlessRawTraceEntry, { kind: "event" }>>();

  for (const event of sortedEvents) {
    const eventType = normalizeText(event.type);
    const data = event.data && typeof event.data === "object" ? event.data as Record<string, unknown> : {};
    const order = entries.length;
    if (eventType.startsWith("headless.message.")) {
      const streamId = normalizeText(data.stream_id) || normalizeText(event.id) || `stream:${order}`;
      const streamPhase = normalizeText(data.phase).toLowerCase();
      const delta = String(data.delta || "");
      const text = String(data.text || "");
      const existing = messageEntryByStreamId.get(streamId);
      if (!existing) {
        const created: Extract<HeadlessRawTraceEntry, { kind: "message" }> = {
          kind: "message",
          id: `message:${streamId}`,
          order,
          ts: eventTimestamp(event),
          streamId,
          streamPhase,
          text: text || delta,
          completed: eventType === "headless.message.completed",
          live: eventType !== "headless.message.completed",
        };
        messageEntryByStreamId.set(streamId, created);
        entries.push(created);
        continue;
      }
      if (streamPhase) existing.streamPhase = streamPhase;
      const nextTs = eventTimestamp(event);
      if (nextTs) existing.ts = nextTs;
      existing.order = order;
      if (text) {
        existing.text = text;
      } else if (delta) {
        existing.text += delta;
      }
      existing.completed = eventType === "headless.message.completed";
      existing.live = eventType !== "headless.message.completed";
      continue;
    }

    const nextEntry = buildEventEntry(event, order);
    if (!nextEntry || nextEntry.kind !== "event") continue;
    const existingEvent = eventEntryById.get(nextEntry.id);
    if (existingEvent) {
      const nextTs = normalizeText(nextEntry.ts);
      if (nextTs) existingEvent.ts = nextTs;
      existingEvent.eventType = nextEntry.eventType;
      existingEvent.badge = nextEntry.badge;
      existingEvent.title = nextEntry.title;
      existingEvent.detailLines = nextEntry.detailLines;
      existingEvent.tone = nextEntry.tone;
      existingEvent.live = nextEntry.live;
      continue;
    }
    eventEntryById.set(nextEntry.id, nextEntry);
    entries.push(nextEntry);
  }

  return entries.sort((left, right) => left.order - right.order);
}
