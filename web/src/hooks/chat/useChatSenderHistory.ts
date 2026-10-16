import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fetchLedgerStatuses, searchChatMessages } from "../../services/api";
import { useGroupStore } from "../../stores";
import type { LedgerEvent, LedgerEventStatusPayload } from "../../types";
import { mergeEventWithExistingStatus } from "../../utils/mergeLedgerEvents";

type Page = { before: string; hasMore: boolean; loaded: boolean; loading: boolean; error: string };
type Direction = "older" | "forward" | "receipts";
type Visit = {
  groupId: string;
  senders: string[];
  key: string;
  page: Page;
  pending: boolean;
  forwardCursor: string;
  handledRevision: number;
  receiptIds: string[];
  retry: Direction | null;
  controller: AbortController;
};
const EMPTY: Page = { before: "", hasMore: true, loaded: false, loading: false, error: "" };
const EMPTY_EVENTS: LedgerEvent[] = [];

/** These receipt records contain scalar fields. Preserve only fields changed by a later live fact. */
function keepLiveFields<T extends object>(
  snapshot: T | undefined,
  before: T | undefined,
  live: T | undefined,
): T | undefined {
  if (!live) return snapshot;
  const next = { ...(snapshot ?? live) };
  for (const key of Object.keys(live) as (keyof T)[])
    if (live[key] !== before?.[key]) next[key] = live[key];
  return next;
}

function keepLiveReceipts(
  snapshot: LedgerEventStatusPayload,
  before: LedgerEvent,
  live: LedgerEvent,
): LedgerEventStatusPayload {
  return {
    ...snapshot,
    connect_delivery: keepLiveFields(
      snapshot.connect_delivery,
      before._connect_delivery,
      live._connect_delivery,
    ),
    connect_cancellation: keepLiveFields(
      snapshot.connect_cancellation,
      before._connect_cancellation,
      live._connect_cancellation,
    ),
    obligation_status:
      snapshot.obligation_status &&
      Object.fromEntries(
        Object.entries(snapshot.obligation_status).map(([actor, status]) => {
          const prior = before._obligation_status?.[actor];
          const current = live._obligation_status?.[actor];
          const next = keepLiveFields(status, prior, current)!;
          if (
            current &&
            (current.replied !== prior?.replied || current.cancelled !== prior?.cancelled)
          ) {
            next.replied = current.replied;
            next.cancelled = current.cancelled;
          }
          return [actor, next];
        }),
      ),
  };
}

/** Sparse history has independent cursors; only daemon reads establish new rows' ledger order. */
export function useChatSenderHistory(groupId: string, senders: readonly string[]) {
  const senderKey = JSON.stringify([...new Set(senders)].sort());
  const senderIds = useMemo<string[]>(() => JSON.parse(senderKey), [senderKey]);
  const key = JSON.stringify([groupId, senderIds]);
  const history = useGroupStore((state) => state.chatByGroup[groupId]?.senderHistory);
  const [state, setState] = useState<{ key: string; page: Page }>({ key, page: EMPTY });
  const current = useRef<Visit | null>(null);
  const currentKey = useRef(key);
  currentKey.current = key;

  const read = useCallback(async (visit: Visit, direction: Direction = "older") => {
    if (!visit.groupId || !visit.senders.length || visit.pending) return;
    if (direction === "older" && !visit.page.hasMore) return;
    const active = () =>
      current.current === visit &&
      currentKey.current === visit.key &&
      !visit.controller.signal.aborted;
    const projection = () => {
      const value = useGroupStore.getState().chatByGroup[visit.groupId]?.senderHistory;
      return active() && value?.key === visit.key ? value : null;
    };
    const initial = projection();
    if (!initial) return;
    visit.pending = true;
    visit.retry = null;
    visit.page = { ...visit.page, loading: direction !== "forward", error: "" };
    setState({ key: visit.key, page: visit.page });
    let retry: Direction = direction;
    try {
      if (direction !== "receipts") {
        const revision = initial.messageRevision;
        const receiptRevision = initial.receiptRevision;
        const forward = direction === "forward" && !!visit.forwardCursor;
        const newest = direction === "forward" ? !visit.forwardCursor : !visit.page.before;
        let after = forward ? visit.forwardCursor : "";
        const received: LedgerEvent[] = [];
        let hasMore = false;
        do {
          const response = await searchChatMessages(
            visit.groupId,
            "",
            forward
              ? { byAny: visit.senders, after, limit: 200 }
              : {
                  byAny: visit.senders,
                  before: (direction === "older" ? visit.page.before : "") || undefined,
                  limit: 50,
                },
            { signal: visit.controller.signal },
          );
          if (!projection()) return;
          if (!response.ok) throw new Error(response.error?.message || "Request failed");
          const events = response.result.events;
          received.push(...events);
          hasMore = events.length > 0 && response.result.has_more;
          if (forward && hasMore && (!events.at(-1)?.id || events.at(-1)?.id === after))
            throw new Error("Ledger cursor did not advance");
          after = events.at(-1)?.id || after;
        } while (forward && hasMore);

        const store = useGroupStore.getState();
        const latest = projection();
        if (!latest) return;
        const bucket = store.chatByGroup[visit.groupId];
        const cached = new Map(
          [...bucket.events, ...latest.events].map((event) => [event.id, event]),
        );
        const incoming = received
          .filter(
            (event) => event.kind === "chat.message" && visit.senders.includes(event.by ?? ""),
          )
          .map((event) => {
            const live = cached.get(event.id);
            return live
              ? mergeEventWithExistingStatus(
                  { ...event, data: { ...event.data, ...live.data } },
                  live,
                )
              : event;
          });
        const updates = new Map(incoming.map((event) => [event.id, event]));
        const existing = latest.events.map((event) => updates.get(event.id) ?? event);
        const known = new Set(existing.map((event) => event.id));
        const added = incoming.filter((event) => !known.has(event.id));
        const events = direction === "older" ? [...added, ...existing] : [...existing, ...added];
        store.setSenderHistory({ ...latest, events }, visit.groupId);

        if (direction === "older" || newest) {
          visit.page = { ...visit.page, before: received[0]?.id || visit.page.before, hasMore };
        }
        if (direction === "forward" || newest) {
          visit.forwardCursor = received.at(-1)?.id || visit.forwardCursor;
          visit.handledRevision = revision;
        }
        visit.page = { ...visit.page, loaded: true };
        if (latest.receiptRevision !== receiptRevision) {
          visit.receiptIds = incoming.map((event) => String(event.id || "")).filter(Boolean);
        }
      }

      // Publish rows first, so subsequent live facts also reach them. If a fact
      // arrived while the page was pending, refresh those IDs once, not by polling.
      retry = "receipts";
      for (let start = 0; start < visit.receiptIds.length; start += 1000) {
        const beforeRead = projection();
        if (!beforeRead) return;
        const response = await fetchLedgerStatuses(
          visit.groupId,
          visit.receiptIds.slice(start, start + 1000),
          { noCache: true, signal: visit.controller.signal },
        );
        const latest = projection();
        if (!latest) return;
        if (!response.ok) throw new Error(response.error?.message || "Request failed");
        const observed = new Map(beforeRead.events.map((event) => [event.id, event]));
        const current = new Map(latest.events.map((event) => [event.id, event]));
        const statuses = Object.fromEntries(
          Object.entries(response.result.statuses).map(([id, status]) => {
            const before = observed.get(id);
            const live = current.get(id);
            return [id, before && live ? keepLiveReceipts(status, before, live) : status];
          }),
        );
        useGroupStore.getState().mergeEventStatuses(statuses, visit.groupId);
      }
      visit.receiptIds = [];
      visit.page = { ...visit.page, loading: false, error: "" };
    } catch (error) {
      if (!active()) return;
      visit.retry = retry;
      visit.page = {
        ...visit.page,
        loaded: true,
        loading: false,
        error: String(error instanceof Error ? error.message : error),
      };
    } finally {
      visit.pending = false;
      if (active()) setState({ key: visit.key, page: visit.page });
    }
  }, []);

  useEffect(() => {
    const visit: Visit = {
      groupId,
      senders: senderIds,
      key,
      page: EMPTY,
      pending: false,
      forwardCursor: "",
      handledRevision: 0,
      receiptIds: [],
      retry: null,
      controller: new AbortController(),
    };
    current.current = visit;
    setState({ key, page: EMPTY });
    if (groupId && senderIds.length)
      useGroupStore
        .getState()
        .setSenderHistory(
          { key, senders: senderIds, events: [], messageRevision: 0, receiptRevision: 0 },
          groupId,
        );
    void read(visit);
    return () => {
      visit.controller.abort();
      if (useGroupStore.getState().chatByGroup[groupId]?.senderHistory?.key === key)
        useGroupStore.getState().setSenderHistory(null, groupId);
    };
  }, [groupId, senderIds, key, read]);

  const page = state.key === key ? state.page : EMPTY;
  const messageRevision = history?.key === key ? history.messageRevision : 0;
  useEffect(() => {
    const visit = current.current;
    if (visit?.key === key && page.loaded && !page.error && messageRevision > visit.handledRevision)
      void read(visit, "forward");
  }, [key, messageRevision, page, read]);

  const loadMore = useCallback(async () => {
    const visit = current.current;
    if (visit?.key === key) await read(visit, visit.retry ?? "older");
  }, [key, read]);

  return {
    events: history?.key === key ? history.events : EMPTY_EVENTS,
    hasMore: senderIds.length > 0 && !page.error && page.hasMore,
    loaded: senderIds.length === 0 || page.loaded,
    loading: senderIds.length > 0 && (page.loading || (!page.loaded && !page.error)),
    error: page.error,
    loadMore,
  };
}
