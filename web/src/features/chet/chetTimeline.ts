import type {
  ChetGroupContext,
  ChetGroupInput,
  ChetGroupSource,
  ChetOutcome,
  ChetRecord,
  ChetSources,
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
  /** A Group message waiting for Chet's turn; nothing has been submitted yet. */
  | "waiting"
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
  /**
   * A text input is the user's words; a voice delegation is the voice model's request; a
   * Group input is a message someone in a Group sent Chet.
   */
  source: "text" | "voice" | "group";
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
  /** For a Group input: where it is, and the message as its Group shows it now, if read. */
  group: { input: ChetGroupInput; message: ChetGroupSource | null } | null;
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
  /**
   * Requests the last status from a free owner reported live. A busy or failed read keeps
   * the previous answer, so an entry placed by it does not move with each check.
   */
  settledLive: ReadonlySet<string>;
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
export function liveIds(status: ChetStatus | null): string[] {
  return status ? [...status.preparing, ...status.queued, ...status.working] : [];
}

export type RequestRecord = Extract<
  ChetRecord,
  { kind: "input" | "voice_delegation" | "group_input" }
>;

export const isRequestRecord = (record: ChetRecord): record is RequestRecord =>
  record.kind === "input" || record.kind === "voice_delegation" || record.kind === "group_input";

/** Requests that `records` or live status name but `records` does not contain. */
export function missingRequestIds(
  records: readonly ChetRecord[],
  status: ChetStatus | null = null,
): string[] {
  const known = new Set<string>();
  const named = liveIds(status);
  for (const record of records) {
    if (isRequestRecord(record)) known.add(record.id);
    else if (record.kind === "result") named.push(...record.input_ids);
    else if (
      record.kind === "not_started" ||
      record.kind === "receipt" ||
      record.kind === "group_dispatch"
    ) {
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
 *
 * A voice delegation is the voice model's wording, saved when the model hands work off, which
 * is often before the transcript of what the user said. So it is shown where its latest loaded
 * fact is (its result, refusal or receipt, else the delegation itself), and after everything
 * while it is settled live work. This ties it to no particular transcript.
 *
 * A Group input waits for Chet's turn until the host submits it. A dispatch or receipt in
 * history shows the submission, and so does status, which names only submitted work. Loaded
 * pages run up to the newest record, so for an input on them, a dispatch they lack has not
 * been written yet; an input read separately is shown only when a later fact names it.
 */
export function buildChetTimeline(
  records: readonly ChetRecord[],
  view: ChetLiveView,
  local: readonly ChetLocalRequest[] = [],
  fetched: ReadonlyMap<string, RequestRecord> = new Map(),
  groupMessages: ChetSources = new Map(),
): ChetTimelineItem[] {
  const sources = new Map<string, { record: RequestRecord; index: number }>();
  const receipts = new Map<string, number>();
  const dispatches = new Map<string, number>();
  const results = new Map<
    string,
    { record: Extract<ChetRecord, { kind: "result" }>; index: number }
  >();
  const notStarted = new Map<string, { reason: string; index: number }>();
  records.forEach((record, index) => {
    if (isRequestRecord(record)) {
      if (!sources.has(record.id)) sources.set(record.id, { record, index });
    } else if (record.kind === "receipt") {
      if (!receipts.has(record.input_id)) receipts.set(record.input_id, index);
    } else if (record.kind === "group_dispatch") {
      if (!dispatches.has(record.input_id)) dispatches.set(record.input_id, index);
    } else if (record.kind === "result") {
      for (const id of record.input_ids) if (!results.has(id)) results.set(id, { record, index });
    } else if (record.kind === "not_started") {
      if (!notStarted.has(record.input_id)) {
        notStarted.set(record.input_id, { reason: record.reason, index });
      }
    }
  });
  const sourceOf = (id: string) => sources.get(id)?.record || fetched.get(id) || null;
  const named = new Set(liveIds(view.status));

  const requestItem = (id: string, source: RequestRecord | null): ChetRequestItem => {
    const result = results.get(id)?.record;
    const reason = notStarted.get(id)?.reason;
    const live = result || reason !== undefined ? null : liveState(id, view);
    const message = groupMessages.get(id) ?? null;
    // Status cannot rule out work it was never given, so it says nothing about a waiting input.
    const waiting =
      source?.kind === "group_input" && !dispatches.has(id) && !receipts.has(id) && !named.has(id);
    return {
      kind: "request",
      id,
      source:
        source?.kind === "voice_delegation"
          ? "voice"
          : source?.kind === "group_input"
            ? "group"
            : "text",
      createdAt: source?.created_at || "",
      text: !source
        ? ""
        : source.kind === "input"
          ? source.request.text
          : source.kind === "voice_delegation"
            ? source.delegation.text
            : message?.status === "available"
              ? message.text
              : "",
      context: source?.kind === "input" ? source.request.context : null,
      state: result
        ? TERMINAL[result.outcome.status] || "ended"
        : reason !== undefined
          ? "not_started"
          : waiting
            ? "waiting"
            : live || (receipts.has(id) ? "received_unconfirmed" : "unconfirmed"),
      outcome: result?.outcome || null,
      resultAt: result?.created_at || "",
      outcomeShownLater: false,
      notStartedReason: reason || "",
      sourceMissing: !source,
      group: source?.kind === "group_input" ? { input: source.input, message } : null,
    };
  };

  const items: ChetTimelineItem[] = [];
  const settledLast: ChetRequestItem[] = [];
  const shown = new Set<string>();
  /** Shows a request at the record at `index` if that is its place. */
  const place = (id: string, index: number) => {
    if (shown.has(id)) return;
    const source = sourceOf(id);
    if (source?.kind === "voice_delegation") {
      const latest =
        results.get(id)?.index ??
        notStarted.get(id)?.index ??
        receipts.get(id) ??
        sources.get(id)?.index;
      if (latest !== index) return;
      shown.add(id);
      const settled = !results.has(id) && !notStarted.has(id) && view.settledLive.has(id);
      (settled ? settledLast : items).push(requestItem(id, source));
    } else if (!sources.has(id) || sources.get(id)?.index === index) {
      shown.add(id);
      items.push(requestItem(id, source));
    }
  };
  records.forEach((record, index) => {
    if (isRequestRecord(record)) {
      place(record.id, index);
    } else if (record.kind === "result") {
      for (const id of record.input_ids) place(id, index);
    } else if (
      record.kind === "not_started" ||
      record.kind === "receipt" ||
      record.kind === "group_dispatch"
    ) {
      place(record.input_id, index);
    } else if (record.kind === "transcript") {
      // The host keeps one record per turn.
      const { transcript } = record;
      const key = `${transcript.call_id}\n${transcript.turn_id}`;
      if (shown.has(key)) return;
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
  });
  items.push(...settledLast);

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
      group: null,
    });
  }

  // Live work whose own records are all on earlier pages.
  for (const id of liveIds(view.status)) {
    if (shown.has(id)) continue;
    shown.add(id);
    items.push(requestItem(id, sourceOf(id)));
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
      ["sending", "preparing", "queued", "working", "checking", "waiting"].includes(item.state),
  );
}

/** Requests whose live work this browser or the host has confirmed. */
export function hasLiveWork(items: readonly ChetTimelineItem[]): boolean {
  return items.some(
    (item) =>
      item.kind === "request" && ["sending", "preparing", "queued", "working"].includes(item.state),
  );
}
