import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CHET_ID_CONFLICT,
  chetSubmissionRefused,
  fetchChetHistory,
  fetchChetHistoryAfter,
  fetchChetInput,
  fetchChetStatus,
  submitChetInput,
  type ChetGroupContext,
  type ChetRecord,
  type ChetSources,
  type ChetStatus,
  type ChetTextRequest,
} from "../../services/api/chet";
import {
  processRecoveryOwnersFromDetails,
  type ProcessRecoveryOwner,
} from "../../services/api/processRecovery";
import { newClientId } from "../../utils/clientId";
import { resendId } from "./chetResend";
import {
  CHET_DRAFT_KEY,
  CHET_PENDING_PREFIX,
  CHET_SEEN_KEY,
  loadChetDraft,
  loadChetPending,
  forgetChetSeen,
  loadChetSeen,
  parseChetPending,
  removeChetPending,
  saveChetDraft,
  saveChetPending,
  saveChetSeen,
  type ChetPendingRequest,
} from "./chetDraft";
import {
  buildChetTimeline,
  hasLiveWork,
  hasOpenRequests,
  isRequestRecord,
  liveIds,
  missingRequestIds,
  type ChetLiveView,
  type ChetLocalRequest,
  type RequestRecord,
} from "./chetTimeline";

const PAGE_SIZE = 50;
const BUSY_POLL_MS = 1_500;
const IDLE_POLL_MS = 5_000;
const CLOSED_POLL_MS = 3_000;
/** A closed page with no known work still reads, slowly, so its entry can show what is new. */
const CLOSED_IDLE_POLL_MS = 15_000;
/** History read on per status read while closed; the next read goes on from there. */
const SCAN_PAGES = 20;

/** What the entry marks as new: someone in a Group asked, an answer came, or a request did not start. */
const NOTEWORTHY: ReadonlySet<ChetRecord["kind"]> = new Set([
  "group_input",
  "result",
  "not_started",
]);

/** A refusal before registration: the message came back to the composer. */
export type ChetComposerProblem = {
  code: string;
  message: string;
  recoveryOwners: ProcessRecoveryOwner[];
};

/** Why a sent request is still held by this browser. */
export type ChetSendProblem =
  | { kind: "unknown"; code: string; message: string }
  /** A retry was refused, but the earlier attempt may still have registered. */
  | { kind: "retry_refused"; code: string; message: string; recoveryOwners: ProcessRecoveryOwner[] }
  | { kind: "conflict"; code: string; message: string };

function mergeAppended(current: ChetRecord[], appended: ChetRecord[]): ChetRecord[] {
  if (!appended.length) return current;
  const known = new Set(current.map((record) => record.id));
  const fresh = appended.filter((record) => !known.has(record.id));
  return fresh.length ? [...current, ...fresh] : current;
}

function requestIds(records: readonly ChetRecord[]): Set<string> {
  return new Set(
    records
      .filter((record) => record.kind === "input" || record.kind === "voice_delegation")
      .map((record) => record.id),
  );
}

function storageWorks(): boolean {
  try {
    // Outside the pending prefix, so other tabs do not take the probe for a send.
    const probe = "cccc.chet.storage-probe";
    window.localStorage.setItem(probe, "1");
    window.localStorage.removeItem(probe);
    return true;
  } catch {
    return false;
  }
}

function newEntry(request: ChetTextRequest, id = newClientId()): ChetPendingRequest {
  return { id, request, state: "unknown", createdAt: new Date().toISOString() };
}

function appendText(current: string, text: string): string {
  return current.trim() ? `${current.replace(/\s+$/, "")}\n\n${text}` : text;
}

/**
 * The global assistant's conversation for this page: durable history from the host, live
 * status, and what this browser holds itself (the draft and sends history does not show
 * yet). Reading never starts an assistant; only a send does.
 */
export function useChetConversation({
  enabled,
  open,
  followClosed = false,
  currentContext,
  onRegistered,
}: {
  enabled: boolean;
  open: boolean;
  /** Read new history while the conversation is closed, so its entry can mark it. */
  followClosed?: boolean;
  /** The group open when a message is sent; it travels with the message as a label. */
  currentContext: ChetGroupContext | null;
  onRegistered?: () => void;
}) {
  const [records, setRecords] = useState<ChetRecord[]>([]);
  const [skipped, setSkipped] = useState(0);
  const [before, setBefore] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [historyError, setHistoryError] = useState("");
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [status, setStatus] = useState<ChetStatus | null>(null);
  const [appliedStatusSeq, setAppliedStatusSeq] = useState(0);
  /** The last usable status reported live work, which a closed page keeps following. */
  const [liveReported, setLiveReported] = useState(false);
  /** Work the last status from a free owner reported live; see `ChetLiveView.settledLive`. */
  const [settledLive, setSettledLive] = useState<ReadonlySet<string>>(() => new Set());
  const [registeredAt, setRegisteredAt] = useState<ReadonlyMap<string, number>>(() => new Map());
  const [fetched, setFetched] = useState<ReadonlyMap<string, RequestRecord>>(() => new Map());
  /** Group messages that inputs name, as their Groups last showed them. */
  const [groupMessages, setGroupMessages] = useState<ChetSources>(() => new Map());
  /** Facts about an earlier page's sends that history holds beyond the loaded pages. */
  const [found, setFound] = useState<ChetRecord[]>([]);
  /** Where history had been read to with the conversation open, here or in another tab. */
  const [seenCursor, setSeenCursor] = useState(loadChetSeen);
  /** What reading on from `seenCursor` found; `cursor` is where to read on from. */
  const scanRef = useRef<{ from: string; cursor: string } | null>(null);
  const [scan, setScan] = useState<{ from: string; unread: boolean } | null>(null);
  const scanningRef = useRef(false);
  /** For a browser that never had the conversation open: history before the loaded pages. */
  const [earlier, setEarlier] = useState<{ before: string; unread: boolean } | null>(null);
  const earlierKeyRef = useRef("");
  const openRef = useRef(open);
  openRef.current = open;
  const [draft, setDraft] = useState(() => loadChetDraft() ?? "");
  const [pending, setPending] = useState<ReadonlyMap<string, ChetPendingRequest>>(
    () => new Map(loadChetPending().map((entry) => [entry.id, entry])),
  );
  // Updated at once, so a send that finishes before the next render sees its own entry.
  const pendingRef = useRef(pending);
  const resendingRef = useRef(new Set<string>());
  const [sendingIds, setSendingIds] = useState<ReadonlySet<string>>(() => new Set());
  const [problems, setProblems] = useState<ReadonlyMap<string, ChetSendProblem>>(() => new Map());
  const [composerProblem, setComposerProblem] = useState<ChetComposerProblem | null>(null);
  const [duplicateRisk, setDuplicateRisk] = useState(false);
  const [storageAvailable, setStorageAvailable] = useState(storageWorks);

  const afterRef = useRef<string | null>(null);
  const statusSeqRef = useRef(0);
  const tickingRef = useRef(false);
  const mountedRef = useRef(true);
  const lookedUpRef = useRef(new Set<string>());
  /** Numbers each read that can bring Group messages, in the order this page asked. */
  const sourceReadRef = useRef(0);
  /** For each input, the read whose answer about its Group message is shown. */
  const sourceShownRef = useRef(new Map<string, number>());
  /** Requests history has shown this page, kept at once so a late send response sees them. */
  const historyIdsRef = useRef(new Set<string>());
  /** Moves the next read forward, keeping one polling schedule. */
  const readSoonRef = useRef<(() => void) | null>(null);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const recordsRef = useRef(records);
  recordsRef.current = records;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  /** Applies a change to the newest draft, including one another tab just stored. */
  const updateDraft = useCallback((change: (current: string) => string) => {
    const next = change(loadChetDraft() ?? draftRef.current);
    draftRef.current = next;
    setDraft(next);
    if (!saveChetDraft(next)) setStorageAvailable(false);
  }, []);

  const commitPending = useCallback((next: ReadonlyMap<string, ChetPendingRequest>) => {
    pendingRef.current = next;
    setPending(next);
  }, []);

  const putPending = useCallback(
    (entry: ChetPendingRequest) => {
      if (!saveChetPending(entry)) setStorageAvailable(false);
      commitPending(new Map(pendingRef.current).set(entry.id, entry));
    },
    [commitPending],
  );

  const dropPending = useCallback(
    (ids: Iterable<string>) => {
      const drop = new Set([...ids].filter((id) => pendingRef.current.has(id)));
      if (!drop.size) return;
      drop.forEach(removeChetPending);
      commitPending(new Map([...pendingRef.current].filter(([id]) => !drop.has(id))));
      setProblems((previous) =>
        [...drop].some((id) => previous.has(id))
          ? new Map([...previous].filter(([id]) => !drop.has(id)))
          : previous,
      );
    },
    [commitPending],
  );

  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === null) {
        draftRef.current = "";
        setDraft("");
        commitPending(new Map());
        return;
      }
      if (event.key === CHET_DRAFT_KEY) {
        const next = event.newValue ?? "";
        draftRef.current = next;
        setDraft(next);
      } else if (event.key === CHET_SEEN_KEY) {
        // Another tab read further with the conversation open.
        setSeenCursor(event.newValue || null);
      } else if (event.key.startsWith(CHET_PENDING_PREFIX)) {
        const id = event.key.slice(CHET_PENDING_PREFIX.length);
        const entry = parseChetPending(id, event.newValue);
        const next = new Map(pendingRef.current);
        if (entry) next.set(id, entry);
        else next.delete(id);
        commitPending(next);
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [commitPending]);

  const applyStatus = useCallback((seq: number, next: ChetStatus | null) => {
    setStatus(next);
    setAppliedStatusSeq(seq);
    if (!next) return;
    if (!next.owner_busy) setSettledLive(new Set(liveIds(next)));
    // A busy owner reports only preparation; keep the last answer for the rest.
    setLiveReported(
      (previous) =>
        next.preparing.length > 0 ||
        (next.owner_busy ? previous : next.queued.length + next.working.length > 0),
    );
    // A status requested after a registration returned is authoritative for it.
    setRegisteredAt((previous) =>
      [...previous.values()].some((at) => at < seq)
        ? new Map([...previous].filter(([, at]) => at >= seq))
        : previous,
    );
  }, []);

  /**
   * Shows how a read found each Group message, unless a read asked for later already answered
   * for it: reads overlap, and an earlier one that returns last must not undo a newer reply.
   */
  const applySources = useCallback((read: number, sources: ChetSources) => {
    const shown = sourceShownRef.current;
    const fresh = [...sources].filter(([id]) => (shown.get(id) ?? 0) <= read);
    if (!fresh.length) return;
    for (const [id] of fresh) shown.set(id, read);
    setGroupMessages((current) => new Map([...current, ...fresh]));
  }, []);

  const tick = useCallback(async () => {
    if (tickingRef.current) return;
    tickingRef.current = true;
    try {
      const seq = ++statusSeqRef.current;
      const after = afterRef.current;
      // Status first: a finished request whose record is saved leaves live status only after the
      // save, so a history read that follows the status read already includes that record.
      const statusResponse = await fetchChetStatus();
      const read = ++sourceReadRef.current;
      const historyResponse = await (after === null
        ? fetchChetHistory({ limit: PAGE_SIZE })
        : fetchChetHistoryAfter(after));
      if (!mountedRef.current) return;
      applyStatus(seq, statusResponse.ok ? statusResponse.result : null);
      if (!historyResponse.ok) {
        setHistoryError(historyResponse.error.message || historyResponse.error.code);
        return;
      }
      const page = historyResponse.result;
      setHistoryError("");
      applySources(read, page.sources);
      if (after === null) {
        setRecords(page.records);
        setBefore(page.before);
        setSkipped(page.skipped);
        setLoaded(true);
      } else {
        setRecords((current) => mergeAppended(current, page.records));
        if (page.skipped) setSkipped((count) => count + page.skipped);
      }
      afterRef.current = page.after;
      // An open conversation has shown everything up to here.
      if (openRef.current && !page.has_more) setSeenCursor(saveChetSeen(page.after));
      // History shows these requests now; this browser no longer needs to hold them.
      const shown = requestIds(page.records);
      shown.forEach((id) => historyIdsRef.current.add(id));
      dropPending(shown);
      if (page.has_more) window.setTimeout(() => void tick(), 0);
    } finally {
      tickingRef.current = false;
    }
  }, [applySources, applyStatus, dropPending]);

  // Sends stored by an earlier page may be older than the newest history page: ask by ID.
  useEffect(() => {
    if (!enabled) return;
    for (const entry of pendingRef.current.values()) {
      if (lookedUpRef.current.has(entry.id)) continue;
      lookedUpRef.current.add(entry.id);
      void fetchChetInput(entry.id).then((response) => {
        if (!mountedRef.current) return;
        if (!response.ok) {
          // A failed read is no answer; the next status read asks again.
          lookedUpRef.current.delete(entry.id);
          return;
        }
        const { records: facts } = response.result;
        if (facts.some((record) => record.id === entry.id && record.kind === "input")) {
          // Shown from these facts in place of the reminder, until a loaded page holds them.
          setFound((previous) => mergeAppended(previous, facts));
          dropPending([entry.id]);
        } else if (entry.state === "registered") {
          // Accepted earlier but not found now: keep it as unknown rather than lose it.
          putPending({ ...entry, state: "unknown" });
        }
      });
    }
    // A failed lookup is asked again with each status read.
  }, [appliedStatusSeq, dropPending, enabled, pending, putPending]);

  const local = useMemo<ChetLocalRequest[]>(
    () =>
      [...pending.values()].map((entry) => ({
        id: entry.id,
        text: entry.request.text,
        context: entry.request.context,
        createdAt: entry.createdAt,
        phase: sendingIds.has(entry.id)
          ? "sending"
          : problems.get(entry.id)?.kind === "conflict"
            ? "conflict"
            : entry.state,
      })),
    [pending, problems, sendingIds],
  );

  // Found facts follow the loaded pages: they are what this browser was waiting for.
  const shownRecords = useMemo(() => mergeAppended(records, found), [found, records]);

  const items = useMemo(() => {
    const assumedLive = new Set(
      [...registeredAt].filter(([, at]) => at >= appliedStatusSeq).map(([id]) => id),
    );
    const view: ChetLiveView = { status, assumedLive, settledLive };
    return buildChetTimeline(shownRecords, view, local, fetched, groupMessages);
  }, [
    appliedStatusSeq,
    fetched,
    groupMessages,
    local,
    registeredAt,
    settledLive,
    shownRecords,
    status,
  ]);

  // When the successor is visible, the older request should not offer another resend.
  const resentIds = useMemo(() => {
    const requests = items.filter((item) => item.kind === "request");
    const ids = new Set(requests.map((item) => item.id));
    return new Set(
      requests
        .filter((item) => item.state === "not_started" && ids.has(resendId(item.id)))
        .map((item) => item.id),
    );
  }, [items]);

  // A page can start after a request it holds facts about; read such requests by ID once, and
  // again at the next status read if reading failed.
  useEffect(() => {
    if (!enabled) return;
    const missing = missingRequestIds(shownRecords, status).filter(
      (id) => !lookedUpRef.current.has(id),
    );
    for (const id of missing.slice(0, 10)) {
      lookedUpRef.current.add(id);
      const read = ++sourceReadRef.current;
      void fetchChetInput(id).then((response) => {
        if (!mountedRef.current) return;
        if (!response.ok) {
          lookedUpRef.current.delete(id);
          return;
        }
        const source = response.result.records.find(
          (record): record is RequestRecord => isRequestRecord(record) && record.id === id,
        );
        if (source) setFetched((previous) => new Map(previous).set(id, source));
        applySources(read, response.result.sources);
      });
    }
  }, [appliedStatusSeq, applySources, enabled, shownRecords, status]);

  // Polling follows anything status may still change; `working` claims only confirmed work.
  const open_requests = hasOpenRequests(items);
  const working = hasLiveWork(items);
  const workingRef = useRef(open_requests);
  workingRef.current = open_requests;
  // A closed page follows work it knows about closely and, when allowed, the rest of history
  // slowly.
  const following = pending.size > 0 || registeredAt.size > 0 || liveReported;
  const followingRef = useRef(following);
  followingRef.current = following;
  const polling = enabled && (open || following || followClosed);

  useEffect(() => {
    if (!polling) return undefined;
    let cancelled = false;
    let timer = 0;
    const schedule = (delay: number) => {
      window.clearTimeout(timer);
      timer = window.setTimeout(run, delay);
    };
    const run = async () => {
      if (document.visibilityState === "visible") await tick();
      if (cancelled) return;
      // Busy work is read more often; the pace changes at the next read, not by restarting.
      schedule(
        !open
          ? followingRef.current
            ? CLOSED_POLL_MS
            : CLOSED_IDLE_POLL_MS
          : workingRef.current
            ? BUSY_POLL_MS
            : IDLE_POLL_MS,
      );
    };
    readSoonRef.current = () => schedule(0);
    // Opening reads at once.
    schedule(open ? 0 : CLOSED_POLL_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      readSoonRef.current = null;
    };
  }, [open, polling, tick]);

  // Closed, read on from where the open conversation had read to, in the order history was
  // written, to the first thing new. Each status read goes on from where the last one stopped.
  useEffect(() => {
    if (!enabled || !followClosed || open || !seenCursor || scanningRef.current) return;
    const from = seenCursor;
    if (scanRef.current?.from !== from) scanRef.current = { from, cursor: from };
    else if (scan?.from === from && scan.unread) return;
    scanningRef.current = true;
    void (async () => {
      try {
        let unread = false;
        for (let pages = 0; pages < SCAN_PAGES && !unread; pages += 1) {
          const position = scanRef.current;
          if (position?.from !== from) return;
          const response = await fetchChetHistoryAfter(position.cursor);
          if (!mountedRef.current) return;
          if (!response.ok) {
            // A refused cursor names no place in this history, which was made again; any
            // other failure is no answer, and the next status read asks again.
            if (response.error.code === "invalid_args") {
              forgetChetSeen(from);
              setSeenCursor((current) => (current === from ? null : current));
            }
            return;
          }
          unread = response.result.records.some((record) => NOTEWORTHY.has(record.kind));
          scanRef.current = { from, cursor: response.result.after };
          if (!response.result.has_more) break;
        }
        setScan((current) =>
          current?.from === from && current.unread === unread ? current : { from, unread },
        );
      } finally {
        scanningRef.current = false;
      }
    })();
  }, [appliedStatusSeq, enabled, followClosed, open, scan, seenCursor]);

  // Never open here: anything of note in history is new. The loaded pages answer first; past
  // them, read back a page at a time to the first thing of note.
  const loadedNoteworthy = useMemo(
    () => records.some((record) => NOTEWORTHY.has(record.kind)),
    [records],
  );
  useEffect(() => {
    if (!enabled || !followClosed || open || !loaded || seenCursor || loadedNoteworthy) return;
    if (!before || earlierKeyRef.current === before) return;
    const start = before;
    earlierKeyRef.current = start;
    void (async () => {
      let cursor: string | null = start;
      while (cursor) {
        const response = await fetchChetHistory({ before: cursor, limit: PAGE_SIZE });
        if (!mountedRef.current) return;
        if (!response.ok) {
          // A failed read is no answer; the next status read asks again.
          if (earlierKeyRef.current === start) earlierKeyRef.current = "";
          return;
        }
        if (response.result.records.some((record) => NOTEWORTHY.has(record.kind))) {
          setEarlier({ before: start, unread: true });
          return;
        }
        cursor = response.result.before;
      }
      setEarlier({ before: start, unread: false });
    })();
  }, [appliedStatusSeq, before, enabled, followClosed, loaded, loadedNoteworthy, open, seenCursor]);

  const unread =
    !open &&
    loaded &&
    (seenCursor
      ? scan?.from === seenCursor && scan.unread
      : loadedNoteworthy || (earlier?.before === before && earlier.unread));

  const loadOlder = useCallback(async () => {
    if (!before || loadingOlder) return;
    setLoadingOlder(true);
    const read = ++sourceReadRef.current;
    const response = await fetchChetHistory({ before, limit: PAGE_SIZE });
    if (!mountedRef.current) return;
    setLoadingOlder(false);
    if (!response.ok) {
      setHistoryError(response.error.message || response.error.code);
      return;
    }
    const known = new Set(recordsRef.current.map((record) => record.id));
    applySources(read, response.result.sources);
    setRecords((current) => [
      ...response.result.records.filter((record) => !known.has(record.id)),
      ...current,
    ]);
    setBefore(response.result.before);
    if (response.result.skipped) setSkipped((count) => count + response.result.skipped);
  }, [applySources, before, loadingOlder]);

  const setText = useCallback((text: string) => updateDraft(() => text), [updateDraft]);

  const submit = useCallback(
    async (entry: ChetPendingRequest, mode: "draft" | "retry" | "again") => {
      // This page knows its own sends; only sends stored by an earlier page are looked up.
      lookedUpRef.current.add(entry.id);
      // Store the ID first: a page closed mid-request must retry it, not send a second one.
      putPending({ ...entry, state: "unknown" });
      setSendingIds((previous) => new Set(previous).add(entry.id));
      setProblems((previous) => {
        if (!previous.has(entry.id)) return previous;
        const next = new Map(previous);
        next.delete(entry.id);
        return next;
      });
      if (mode !== "retry") {
        setComposerProblem(null);
        setDuplicateRisk(false);
      }
      const response = await submitChetInput(entry.id, entry.request);
      if (!mountedRef.current) return;
      setSendingIds((previous) => {
        const next = new Set(previous);
        next.delete(entry.id);
        return next;
      });
      if (response.ok) {
        setRegisteredAt((previous) => new Map(previous).set(entry.id, statusSeqRef.current));
        // History may have shown it before this response came back.
        if (historyIdsRef.current.has(entry.id)) dropPending([entry.id]);
        else putPending({ ...entry, state: "registered" });
        onRegistered?.();
        if (readSoonRef.current) readSoonRef.current();
        else void tick();
        return;
      }
      const { code, message, details } = response.error;
      const setProblem = (problem: ChetSendProblem) =>
        setProblems((previous) => new Map(previous).set(entry.id, problem));
      if (code === CHET_ID_CONFLICT) {
        setProblem({ kind: "conflict", code, message });
        return;
      }
      if (chetSubmissionRefused(code)) {
        const recoveryOwners =
          code === "codex_voice_recovery_pending" ? processRecoveryOwnersFromDetails(details) : [];
        if (mode === "retry") {
          setProblem({ kind: "retry_refused", code, message, recoveryOwners });
          return;
        }
        // Nothing was registered: the text goes back to the composer.
        dropPending([entry.id]);
        if (mode === "draft") updateDraft((current) => appendText(current, entry.request.text));
        setComposerProblem({ code, message, recoveryOwners });
        return;
      }
      setProblem({ kind: "unknown", code, message });
    },
    [dropPending, onRegistered, putPending, tick, updateDraft],
  );

  /** Sends the draft as a new request. */
  const send = useCallback(() => {
    const text = draftRef.current;
    if (!text.trim()) return;
    const entry = newEntry({ text, context: currentContext });
    // Another tab may have stored a newer draft that this page has not heard about yet.
    // Storage gives no atomic swap, so a write between this read and the clear can still win.
    updateDraft((current) => (current === text ? "" : current));
    void submit(entry, "draft");
  }, [currentContext, submit, updateDraft]);

  /** Retries a send whose registration is unknown. The same ID cannot run twice. */
  const retry = useCallback(
    (id: string) => {
      const entry = pendingRef.current.get(id);
      if (entry && !sendingIds.has(id)) void submit(entry, "retry");
    },
    [sendingIds, submit],
  );

  /** Gives one not-started request a stable successor, even across tabs or reloads. */
  const sendAgain = useCallback(
    (sourceId: string, request: ChetTextRequest) => {
      const id = resendId(sourceId);
      if (
        resendingRef.current.has(id) ||
        pendingRef.current.has(id) ||
        historyIdsRef.current.has(id)
      )
        return;
      resendingRef.current.add(id);
      void submit(newEntry(request, id), "again").finally(() => resendingRef.current.delete(id));
    },
    [submit],
  );

  /**
   * Copies an unknown send back to the composer for editing. The reminder stays until
   * history settles it or the user dismisses it, since the first send may still arrive.
   */
  const putBack = useCallback(
    (id: string) => {
      const entry = pendingRef.current.get(id);
      if (!entry) return;
      updateDraft((current) => appendText(current, entry.request.text));
      setDuplicateRisk(true);
    },
    [updateDraft],
  );

  /** Stops following an unknown send; it may still have reached the assistant. */
  const dismiss = useCallback((id: string) => dropPending([id]), [dropPending]);

  return {
    items,
    loaded,
    skipped,
    historyError,
    hasOlder: Boolean(before),
    loadingOlder,
    loadOlder,
    status,
    draft,
    storageAvailable,
    setText,
    send,
    retry,
    sendAgain,
    resentIds,
    putBack,
    dismiss,
    problems,
    composerProblem,
    clearComposerProblem: () => setComposerProblem(null),
    duplicateRisk,
    working,
    /** History holds something new since the conversation was last open here. */
    unread,
  };
}

export type ChetConversation = ReturnType<typeof useChetConversation>;
