import type {
  ChetGroupContext,
  ChetOutcome,
  ChetRecord,
  ChetStatus,
} from "../../services/api/chet";
import type { VoiceConversationTurn } from "../codexVoice/codexVoiceProtocol";

/**
 * What the user can rely on for one request. Only a result record is a Runtime's
 * terminal state; without one, live status or the lack of it decides what is known.
 */
export type ChetRequestState =
  | "sending"
  /** This browser does not know whether the host registered the send. */
  | "send_unknown"
  /** The ID already names a different request; nothing was sent. */
  | "send_conflict"
  | "preparing"
  | "queued"
  | "working"
  | "checking"
  | "completed"
  | "failed"
  | "cancelled"
  /** The answer was too long to keep in full, so none of it is kept. */
  | "too_large"
  | "ended"
  | "not_started"
  | "received_unconfirmed"
  | "unconfirmed";

export type ChetRequestItem = {
  kind: "request";
  id: string;
  /** A text input is the user's words; a voice delegation is the voice model's request. */
  source: "text" | "voice";
  createdAt: string;
  text: string;
  context: ChetGroupContext | null;
  state: ChetRequestState;
  outcome: ChetOutcome | null;
  resultAt: string;
  /** The outcome is shown with a later request that finished in the same turn. */
  outcomeShownLater: boolean;
  notStartedReason: string;
  /** History shows what followed this request, but not the request itself yet. */
  sourceMissing: boolean;
};

export type ChetTranscriptItem = {
  kind: "transcript";
  id: string;
  createdAt: string;
  callId: string;
  role: "user" | "assistant";
  text: string;
  final: boolean;
  complete: boolean;
  seq: number;
};

export type ChetTimelineItem = ChetRequestItem | ChetTranscriptItem;

/** A request this browser sent that history does not show yet. */
export type ChetLocalRequest = {
  id: string;
  text: string;
  context: ChetGroupContext | null;
  createdAt: string;
  phase: "sending" | "registered" | "unknown" | "conflict";
};

export type ChetLiveView = {
  status: ChetStatus | null;
  /** Requests whose registration returned after the current status was requested. */
  assumedLive: ReadonlySet<string>;
};

const TERMINAL: Record<string, ChetRequestState> = {
  completed: "completed",
  failed: "failed",
  cancelled: "cancelled",
  result_too_large: "too_large",
};

export function liveState(
  id: string,
  view: ChetLiveView,
): "preparing" | "queued" | "working" | "checking" | null {
  const { status } = view;
  if (status?.preparing.includes(id)) return "preparing";
  if (status && !status.owner_busy) {
    if (status.working.includes(id)) return "working";
    if (status.queued.includes(id)) return "queued";
  }
  if (view.assumedLive.has(id)) return "preparing";
  // Without a usable status, or while its owner is busy, nothing can be ruled out yet.
  if (!status || status.owner_busy) return "checking";
  return null;
}

/** Requests status reports as live. */
function liveIds(status: ChetStatus | null): string[] {
  return status ? [...status.preparing, ...status.queued, ...status.working] : [];
}

type RequestRecord = Extract<ChetRecord, { kind: "input" | "voice_delegation" }>;

/** Requests that `records` or live status name but `records` does not contain. */
export function missingRequestIds(
  records: readonly ChetRecord[],
  status: ChetStatus | null = null,
): string[] {
  const known = new Set<string>();
  const named = liveIds(status);
  for (const record of records) {
    if (record.kind === "input" || record.kind === "voice_delegation") known.add(record.id);
    else if (record.kind === "result") named.push(...record.input_ids);
    else if (record.kind === "not_started" || record.kind === "receipt") {
      named.push(record.input_id);
    }
  }
  return [...new Set(named.filter((id) => !known.has(id)))];
}

/**
 * History pages by fact, so a page can hold a receipt or result whose request is on an
 * earlier page, and status can report work none of whose records are loaded. Such a request
 * appears where its first loaded fact is, or after everything when only status names it; its
 * text comes from `fetched` once read separately, and until then it is a placeholder.
 */
export function buildChetTimeline(
  records: readonly ChetRecord[],
  view: ChetLiveView,
  local: readonly ChetLocalRequest[] = [],
  fetched: ReadonlyMap<string, RequestRecord> = new Map(),
): ChetTimelineItem[] {
  const receipts = new Set<string>();
  const results = new Map<string, Extract<ChetRecord, { kind: "result" }>>();
  const notStarted = new Map<string, string>();
  for (const record of records) {
    if (record.kind === "receipt") receipts.add(record.input_id);
    else if (record.kind === "result") {
      for (const id of record.input_ids) if (!results.has(id)) results.set(id, record);
    } else if (record.kind === "not_started") {
      if (!notStarted.has(record.input_id)) notStarted.set(record.input_id, record.reason);
    }
  }

  const requestItem = (id: string, source: RequestRecord | null): ChetRequestItem => {
    const result = results.get(id);
    const reason = notStarted.get(id);
    const live = result || reason !== undefined ? null : liveState(id, view);
    return {
      kind: "request",
      id,
      source: source?.kind === "voice_delegation" ? "voice" : "text",
      createdAt: source?.created_at || "",
      text: !source ? "" : source.kind === "input" ? source.request.text : source.delegation.text,
      context: source?.kind === "input" ? source.request.context : null,
      state: result
        ? TERMINAL[result.outcome.status] || "ended"
        : reason !== undefined
          ? "not_started"
          : live || (receipts.has(id) ? "received_unconfirmed" : "unconfirmed"),
      outcome: result?.outcome || null,
      resultAt: result?.created_at || "",
      outcomeShownLater: false,
      notStartedReason: reason || "",
      sourceMissing: !source,
    };
  };

  const loaded = new Set(
    records
      .filter((record) => record.kind === "input" || record.kind === "voice_delegation")
      .map((record) => record.id),
  );
  const items: ChetTimelineItem[] = [];
  const shown = new Set<string>();
  for (const record of records) {
    if (record.kind === "input" || record.kind === "voice_delegation") {
      if (shown.has(record.id)) continue;
      shown.add(record.id);
      items.push(requestItem(record.id, record));
    } else if (
      record.kind === "result" ||
      record.kind === "not_started" ||
      record.kind === "receipt"
    ) {
      const ids = record.kind === "result" ? record.input_ids : [record.input_id];
      for (const id of ids) {
        if (loaded.has(id) || shown.has(id)) continue;
        shown.add(id);
        items.push(requestItem(id, fetched.get(id) || null));
      }
    } else if (record.kind === "transcript") {
      // The host keeps one record per turn.
      const { transcript } = record;
      const key = `${transcript.call_id}\n${transcript.turn_id}`;
      if (shown.has(key)) continue;
      shown.add(key);
      items.push({
        kind: "transcript",
        id: key,
        createdAt: record.created_at,
        callId: transcript.call_id,
        role: transcript.role,
        text: transcript.text,
        final: transcript.final_update,
        complete: transcript.complete,
        seq: transcript.seq,
      });
    }
  }

  for (const request of local) {
    if (shown.has(request.id)) continue;
    shown.add(request.id);
    items.push({
      kind: "request",
      id: request.id,
      source: "text",
      createdAt: request.createdAt,
      text: request.text,
      context: request.context,
      state:
        request.phase === "sending"
          ? "sending"
          : request.phase === "unknown"
            ? "send_unknown"
            : request.phase === "conflict"
              ? "send_conflict"
              : liveState(request.id, view) || "preparing",
      outcome: null,
      resultAt: "",
      outcomeShownLater: false,
      notStartedReason: "",
      sourceMissing: false,
    });
  }

  // Live work whose own records are all on earlier pages.
  for (const id of liveIds(view.status)) {
    if (shown.has(id)) continue;
    shown.add(id);
    items.push(requestItem(id, fetched.get(id) || null));
  }

  // One turn can answer several requests, and each gets its own result record. Show the
  // answer once, after the last of them.
  const lastForTurn = new Map<string, number>();
  items.forEach((item, index) => {
    if (item.kind === "request" && item.outcome) {
      lastForTurn.set(`${item.outcome.generation}\n${item.outcome.turn_id}`, index);
    }
  });
  items.forEach((item, index) => {
    if (item.kind !== "request" || !item.outcome) return;
    const key = `${item.outcome.generation}\n${item.outcome.turn_id}`;
    if (lastForTurn.get(key) !== index) item.outcomeShownLater = true;
  });

  return orderTranscriptRuns(items);
}

/**
 * Input transcription can finish after the reply it prompted, so a call's adjacent
 * transcripts are shown in the order the call first saw them.
 */
function orderTranscriptRuns(items: ChetTimelineItem[]): ChetTimelineItem[] {
  const ordered = [...items];
  let start = 0;
  while (start < ordered.length) {
    const first = ordered[start];
    if (first.kind !== "transcript") {
      start += 1;
      continue;
    }
    let end = start + 1;
    while (end < ordered.length) {
      const next = ordered[end];
      if (next.kind !== "transcript" || next.callId !== first.callId) break;
      end += 1;
    }
    const run = ordered.slice(start, end) as ChetTranscriptItem[];
    run.sort((left, right) => left.seq - right.seq);
    ordered.splice(start, run.length, ...run);
    start = end;
  }
  return ordered;
}

/** The call's turns that history does not hold yet; a saved turn shows once, from history. */
export function unsavedLiveTurns(
  items: readonly ChetTimelineItem[],
  turns: readonly VoiceConversationTurn[],
): readonly VoiceConversationTurn[] {
  if (!turns.length) return turns;
  const saved = new Set(items.filter((item) => item.kind === "transcript").map((item) => item.id));
  return turns.filter((turn) => !turn.callId || !saved.has(`${turn.callId}\n${turn.id}`));
}

/** Requests that live status may still change. */
export function hasOpenRequests(items: readonly ChetTimelineItem[]): boolean {
  return items.some(
    (item) =>
      item.kind === "request" &&
      ["sending", "preparing", "queued", "working", "checking"].includes(item.state),
  );
}

/** Requests whose live work this browser or the host has confirmed. */
export function hasLiveWork(items: readonly ChetTimelineItem[]): boolean {
  return items.some(
    (item) =>
      item.kind === "request" && ["sending", "preparing", "queued", "working"].includes(item.state),
  );
}
