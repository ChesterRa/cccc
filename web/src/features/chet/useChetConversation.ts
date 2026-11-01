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
  type ChetStatus,
  type ChetTextRequest,
} from "../../services/api/chet";
import {
  processRecoveryOwnersFromDetails,
  type ProcessRecoveryOwner,
} from "../../services/api/processRecovery";
import { newClientId } from "../../utils/clientId";
import {
  CHET_DRAFT_KEY,
  CHET_PENDING_PREFIX,
  loadChetDraft,
  loadChetPending,
  parseChetPending,
  removeChetPending,
  saveChetDraft,
  saveChetPending,
  type ChetPendingRequest,
} from "./chetDraft";
import {
  buildChetTimeline,
  hasLiveWork,
  hasOpenRequests,
  liveIds,
  missingRequestIds,
  type ChetLiveView,
  type ChetLocalRequest,
} from "./chetTimeline";

const PAGE_SIZE = 50;
const BUSY_POLL_MS = 1_500;
const IDLE_POLL_MS = 5_000;
const CLOSED_POLL_MS = 3_000;

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

type RequestRecord = Extract<ChetRecord, { kind: "input" | "voice_delegation" }>;

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

function newEntry(request: ChetTextRequest): ChetPendingRequest {
  return { id: newClientId(), request, state: "unknown", createdAt: new Date().toISOString() };
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
  currentContext,
  onRegistered,
}: {
  enabled: boolean;
  open: boolean;
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
  /** Facts about an earlier page's sends that history holds beyond the loaded pages. */
  const [found, setFound] = useState<ChetRecord[]>([]);
  const [draft, setDraft] = useState(() => loadChetDraft() ?? "");
  const [pending, setPending] = useState<ReadonlyMap<string, ChetPendingRequest>>(
    () => new Map(loadChetPending().map((entry) => [entry.id, entry])),
  );
  // Updated at once, so a send that finishes before the next render sees its own entry.
  const pendingRef = useRef(pending);
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

  const tick = useCallback(async () => {
    if (tickingRef.current) return;
    tickingRef.current = true;
    try {
      const seq = ++statusSeqRef.current;
      const after = afterRef.current;
      // Status first: a finished request whose record is saved leaves live status only after the
      // save, so a history read that follows the status read already includes that record.
      const statusResponse = await fetchChetStatus();
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
      // History shows these requests now; this browser no longer needs to hold them.
      const shown = requestIds(page.records);
      shown.forEach((id) => historyIdsRef.current.add(id));
      dropPending(shown);
      if (page.has_more) window.setTimeout(() => void tick(), 0);
    } finally {
      tickingRef.current = false;
    }
  }, [applyStatus, dropPending]);

  // Sends stored by an earlier page may be older than the newest history page: ask by ID.
  useEffect(() => {
    if (!enabled) return;
    for (const entry of pendingRef.current.values()) {
      if (lookedUpRef.current.has(entry.id)) continue;
      lookedUpRef.current.add(entry.id);
      void fetchChetInput(entry.id).then((response) => {
        if (!mountedRef.current || !response.ok) return;
        if (response.result.some((record) => record.id === entry.id && record.kind === "input")) {
          // Shown from these facts in place of the reminder, until a loaded page holds them.
          setFound((previous) => mergeAppended(previous, response.result));
          dropPending([entry.id]);
        } else if (entry.state === "registered") {
          // Accepted earlier but not found now: keep it as unknown rather than lose it.
          putPending({ ...entry, state: "unknown" });
        }
      });
    }
  }, [dropPending, enabled, pending, putPending]);

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
    return buildChetTimeline(shownRecords, view, local, fetched);
  }, [appliedStatusSeq, fetched, local, registeredAt, settledLive, shownRecords, status]);

  // A page can start after a request it holds facts about; read such requests by ID once.
  useEffect(() => {
    if (!enabled) return;
    const missing = missingRequestIds(shownRecords, status).filter(
      (id) => !lookedUpRef.current.has(id),
    );
    for (const id of missing.slice(0, 10)) {
      lookedUpRef.current.add(id);
      void fetchChetInput(id).then((response) => {
        if (!mountedRef.current || !response.ok) return;
        const source = response.result.find(
          (record): record is RequestRecord =>
            (record.kind === "input" || record.kind === "voice_delegation") && record.id === id,
        );
        if (source) setFetched((previous) => new Map(previous).set(id, source));
      });
    }
  }, [enabled, shownRecords, status]);

  // Polling follows anything status may still change; `working` claims only confirmed work.
  const open_requests = hasOpenRequests(items);
  const working = hasLiveWork(items);
  const workingRef = useRef(open_requests);
  workingRef.current = open_requests;
  // A closed page only follows work it already knows about.
  const polling = enabled && (open || pending.size > 0 || registeredAt.size > 0 || liveReported);

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
      schedule(!open ? CLOSED_POLL_MS : workingRef.current ? BUSY_POLL_MS : IDLE_POLL_MS);
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

  const loadOlder = useCallback(async () => {
    if (!before || loadingOlder) return;
    setLoadingOlder(true);
    const response = await fetchChetHistory({ before, limit: PAGE_SIZE });
    if (!mountedRef.current) return;
    setLoadingOlder(false);
    if (!response.ok) {
      setHistoryError(response.error.message || response.error.code);
      return;
    }
    const known = new Set(recordsRef.current.map((record) => record.id));
    setRecords((current) => [
      ...response.result.records.filter((record) => !known.has(record.id)),
      ...current,
    ]);
    setBefore(response.result.before);
    if (response.result.skipped) setSkipped((count) => count + response.result.skipped);
  }, [before, loadingOlder]);

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

  /** Sends a request that never started again, as a new request about the same group. */
  const sendAgain = useCallback(
    (request: ChetTextRequest) => void submit(newEntry(request), "again"),
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
    putBack,
    dismiss,
    problems,
    composerProblem,
    clearComposerProblem: () => setComposerProblem(null),
    duplicateRisk,
    working,
  };
}

export type ChetConversation = ReturnType<typeof useChetConversation>;
