import { useCallback, useContext, useEffect, useRef, useState } from "react";
import { fetchLedgerStatuses } from "../../services/api";
import { fetchPendingReplies, type PendingReplyPage } from "../../services/api/pendingReplies";
import { mergeLedgerEventStatuses } from "../../stores/groupStoreCore";
import type { LedgerEvent, LedgerEventStatusPayload } from "../../types";
import { PendingSummaryContext } from "./context";

type PageState = {
  key: string;
  events: LedgerEvent[];
  before: string | null;
  beforeAnchor: string | null;
  hasMore: boolean;
  loaded: boolean;
  loading: boolean;
  error: boolean;
};

type Visit = {
  groupId: string;
  state: PageState;
  controller: AbortController;
  pending: boolean;
  refreshQueued: boolean;
};

function empty(key: string): PageState {
  return {
    key,
    events: [],
    before: null,
    beforeAnchor: null,
    hasMore: false,
    loaded: false,
    loading: false,
    error: false,
  };
}

/** The server orders by ledger position; timestamps cannot safely order these rows. */
function mergeChronological(
  old: LedgerEvent[],
  incoming: LedgerEvent[],
  direction: "head" | "older",
  anchor: string | null,
): LedgerEvent[] {
  if (direction === "older" && anchor) {
    // The cursor belongs to the oldest row of the preceding page, even when
    // earlier rows have been retained on screen after a head refresh.
    const incomingIds = new Set(incoming.map((event) => event.id));
    const remaining = old.filter((event) => !incomingIds.has(event.id));
    const position = remaining.findIndex((event) => event.id === anchor);
    if (position < 0) throw new Error("page anchor missing");
    return [...remaining.slice(0, position), ...incoming, ...remaining.slice(position)];
  }
  const oldIds = new Set(old.map((event) => event.id));
  const replacements = new Map(incoming.map((event) => [event.id, event]));
  let result = old.map((event) => replacements.get(event.id) ?? event);
  let additions: LedgerEvent[] = [];
  let shared = false;
  for (const event of incoming) {
    if (!oldIds.has(event.id)) {
      additions.push(event);
      continue;
    }
    shared = true;
    const index = result.findIndex((current) => current.id === event.id);
    result = [...result.slice(0, index), ...additions, ...result.slice(index)];
    additions = [];
  }
  return !shared && direction === "older" ? [...additions, ...result] : [...result, ...additions];
}

function checkedPage(page: PendingReplyPage, groupId: string): LedgerEvent[] {
  if (
    !page ||
    !Array.isArray(page.events) ||
    !page.statuses ||
    typeof page.statuses !== "object" ||
    typeof page.has_more !== "boolean" ||
    (page.has_more && (typeof page.before !== "string" || !page.before))
  )
    throw new Error("invalid page");
  const seen = new Set<string>();
  const events = page.events.map((event) => {
    if (
      !event ||
      typeof event.id !== "string" ||
      !event.id ||
      seen.has(event.id) ||
      event.kind !== "chat.message" ||
      (event.group_id && event.group_id !== groupId)
    )
      throw new Error("invalid event");
    seen.add(event.id);
    // Never accept status-shaped fields from a ledger record in place of the authoritative status.
    return { ...event, _user_request: undefined, _obligation_status: undefined };
  });
  return mergeLedgerEventStatuses(events, page.statuses);
}

/** A visit is discarded on group/filter change; unsent drafts live in a separate store. */
export function usePendingPages(groupId: string, enabled: boolean) {
  const summary = useContext(PendingSummaryContext);
  const active = !!summary && enabled && !!groupId;
  const key = active ? groupId : "";
  const [state, setState] = useState<PageState>(() => empty(key));
  const visitRef = useRef<Visit | null>(null);
  const currentKey = useRef(key);
  currentKey.current = key;
  const readRef = useRef<
    (visit: Visit, direction: "head" | "older", reset?: boolean) => Promise<void>
  >(async () => {});

  const read = useCallback(async (visit: Visit, direction: "head" | "older", reset = false) => {
    const isCurrent = () =>
      visitRef.current === visit &&
      currentKey.current === visit.groupId &&
      !visit.controller.signal.aborted;
    if (!isCurrent()) return;
    if (visit.pending) {
      if (direction === "head") visit.refreshQueued = true;
      return;
    }
    if (direction === "older" && (!visit.state.hasMore || !visit.state.before)) return;
    visit.pending = true;
    visit.state = { ...visit.state, loading: true, error: false };
    setState(visit.state);
    try {
      const existingIds = new Set(visit.state.events.map((event) => event.id));
      const result = await fetchPendingReplies(
        visit.groupId,
        direction === "older" ? (visit.state.before ?? undefined) : undefined,
        { signal: visit.controller.signal },
      );
      if (!isCurrent()) return;
      if (!result.ok) {
        if (direction === "older" && result.error.code === "cursor_stale") {
          visit.pending = false;
          await readRef.current(visit, "head", true);
          return;
        }
        throw new Error("read failed");
      }
      const page = result.result;
      const received = checkedPage(page, visit.groupId);
      const overlaps = received.some((event) => existingIds.has(event.id));
      if (direction === "older" && page.has_more && page.before === visit.state.before)
        throw new Error("cursor did not advance");

      const incomingIds = new Set(received.map((event) => event.id));
      const missing = visit.state.events.filter((event) => !incomingIds.has(event.id));
      let statuses: Record<string, LedgerEventStatusPayload> = {};
      if (direction === "head" && missing.length) {
        for (let offset = 0; offset < missing.length; offset += 1000) {
          const ids = missing
            .slice(offset, offset + 1000)
            .map((event) => event.id!)
            .filter(Boolean);
          const response = await fetchLedgerStatuses(visit.groupId, ids, {
            signal: visit.controller.signal,
            noCache: true,
          });
          if (!isCurrent()) return;
          if (!response.ok || !response.result?.statuses) throw new Error("status read failed");
          statuses = { ...statuses, ...response.result.statuses };
        }
      }
      const previous =
        direction === "head"
          ? mergeLedgerEventStatuses(
              visit.state.events.map((event) =>
                incomingIds.has(event.id)
                  ? event
                  : { ...event, _user_request: undefined, _obligation_status: undefined },
              ),
              statuses,
            )
          : visit.state.events;
      const events = mergeChronological(previous, received, direction, visit.state.beforeAnchor);
      // If new requests filled an entire page since our last visit, there may
      // be a gap. Restart paging from the new head instead of silently skipping it.
      const cursorPage =
        direction === "older" || !visit.state.loaded || reset || !overlaps ? page : null;
      visit.state = {
        ...visit.state,
        events,
        before: cursorPage ? (cursorPage.before ?? null) : visit.state.before,
        beforeAnchor: cursorPage ? (received[0]?.id ?? null) : visit.state.beforeAnchor,
        hasMore: cursorPage ? cursorPage.has_more : visit.state.hasMore,
        loaded: true,
        loading: false,
        error: false,
      };
      setState(visit.state);
    } catch {
      if (!isCurrent()) return;
      // Unverified old rows are retained for context, but cannot offer an answer.
      visit.state = {
        ...visit.state,
        events: visit.state.events.map((event) => ({
          ...event,
          _user_request: undefined,
          _obligation_status: undefined,
        })),
        loading: false,
        error: true,
      };
      setState(visit.state);
    } finally {
      if (isCurrent()) {
        visit.pending = false;
        if (visit.refreshQueued) {
          visit.refreshQueued = false;
          void readRef.current(visit, "head");
        }
      }
    }
  }, []);
  readRef.current = read;

  useEffect(() => {
    if (!active) {
      visitRef.current = null;
      setState(empty(""));
      return;
    }
    const visit: Visit = {
      groupId,
      state: empty(groupId),
      controller: new AbortController(),
      pending: false,
      refreshQueued: false,
    };
    visitRef.current = visit;
    setState(visit.state);
    void readRef.current(visit, "head");
    return () => {
      visit.controller.abort();
      if (visitRef.current === visit) visitRef.current = null;
    };
  }, [active, groupId]);

  const revision = summary?.summaries[groupId]?.revision;
  const stateKind = summary?.summaries[groupId]?.state;
  const lastRevision = useRef<string | undefined>(undefined);
  useEffect(() => {
    const visit = visitRef.current;
    if (!visit || visit.groupId !== groupId || stateKind !== "fresh" || !revision) return;
    const revisionKey = JSON.stringify([groupId, revision]);
    if (lastRevision.current === revisionKey) return;
    lastRevision.current = revisionKey;
    void readRef.current(visit, "head");
  }, [groupId, revision, stateKind]);

  const loadMore = useCallback(() => {
    const visit = visitRef.current;
    if (visit) void readRef.current(visit, "older");
  }, []);
  const refresh = useCallback(() => {
    const visit = visitRef.current;
    if (visit) void readRef.current(visit, "head");
  }, []);
  return { ...(state.key === key ? state : empty(key)), active, loadMore, refresh };
}
