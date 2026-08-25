import * as api from "../../services/api";
import { useGroupStore } from "../../stores";
import type { LedgerEvent } from "../../types";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "../../utils/chatOutboxReconciliation";
import { mergeLedgerEvents, projectCrossGroupReceipts } from "../../utils/mergeLedgerEvents";

const MAX_RECONCILED_EVENTS = 800;
const RECONNECT_LEDGER_TAIL_LIMIT = 60;
const RECONNECT_FORWARD_PAGE_LIMIT = 200;

type LedgerCatchup = { cursor: string; events: LedgerEvent[]; hasMoreHistory: boolean | null };

function lastEventId(events: readonly { id?: unknown }[]): string {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const eventId = String(events[index]?.id || "").trim();
    if (eventId) return eventId;
  }
  return "";
}

function mergeForwardLedgerEvents(
  existing: LedgerEvent[],
  incoming: LedgerEvent[],
  anchor: string,
): LedgerEvent[] {
  if (incoming.length === 0) return existing;
  const existingById = new Map(
    existing
      .map((event) => [String(event.id || "").trim(), event] as const)
      .filter(([eventId]) => eventId),
  );
  const hydratedIncoming = incoming.map((event) => {
    const current = existingById.get(String(event.id || "").trim());
    return current
      ? {
          ...event,
          _read_status: event._read_status ?? current._read_status,
          _obligation_status: event._obligation_status ?? current._obligation_status,
        }
      : event;
  });
  const incomingIds = new Set(
    hydratedIncoming.map((event) => String(event.id || "").trim()).filter(Boolean),
  );
  const anchorIndex = existing.findIndex((event) => String(event.id || "").trim() === anchor);
  const prefix = anchorIndex >= 0 ? existing.slice(0, anchorIndex + 1) : existing;
  const suffix = anchorIndex >= 0 ? existing.slice(anchorIndex + 1) : [];
  const combined = [
    ...prefix.filter((event) => !incomingIds.has(String(event.id || "").trim())),
    ...hydratedIncoming,
    ...suffix.filter((event) => !incomingIds.has(String(event.id || "").trim())),
  ];
  const projected = projectCrossGroupReceipts(combined);
  return projected.length > MAX_RECONCILED_EVENTS
    ? projected.slice(projected.length - MAX_RECONCILED_EVENTS)
    : projected;
}

async function loadTailSnapshot(groupId: string): Promise<LedgerCatchup | null> {
  const [tail, boundary] = await Promise.all([
    api.fetchLedgerTail(groupId, RECONNECT_LEDGER_TAIL_LIMIT, {
      includeStatuses: false,
      cache: "no-store",
    }),
    api.fetchLedgerBoundary(groupId),
  ]);
  if (!tail.ok) return null;
  const boundaryEvents = boundary.ok ? boundary.result.events || [] : [];
  return {
    cursor: lastEventId(boundaryEvents) || lastEventId(tail.result.events || []),
    events: tail.result.events || [],
    hasMoreHistory: !!tail.result.has_more,
  };
}

async function loadForwardCatchup(
  groupId: string,
  afterEventId: string,
): Promise<LedgerCatchup | null> {
  const boundary = await api.fetchLedgerBoundary(groupId);
  if (!boundary.ok) return null;
  const boundaryId = lastEventId(boundary.result.events || []);
  if (!boundaryId || boundaryId === afterEventId) {
    return { cursor: boundaryId || afterEventId, events: [], hasMoreHistory: null };
  }

  let cursor = afterEventId;
  const events: LedgerCatchup["events"] = [];
  for (;;) {
    const response = await api.searchChatMessages(groupId, "", {
      after: cursor,
      limit: RECONNECT_FORWARD_PAGE_LIMIT,
      includeStatuses: false,
    });
    if (!response.ok) {
      if (response.error.code === "event_not_found") return loadTailSnapshot(groupId);
      return null;
    }
    const page = response.result.events || [];
    events.push(...page);
    if (!response.result.has_more) break;
    const nextCursor = lastEventId(page);
    if (!nextCursor || nextCursor === cursor) return loadTailSnapshot(groupId);
    cursor = nextCursor;
  }
  return { cursor: lastEventId(events) || boundaryId, events, hasMoreHistory: null };
}

export async function reconcileLedgerTail(
  groupId: string,
  isCurrentGroup: () => boolean,
  afterEventId = "",
): Promise<string> {
  const anchor = String(afterEventId || "").trim();
  const catchup = anchor
    ? await loadForwardCatchup(groupId, anchor)
    : await loadTailSnapshot(groupId);
  if (!catchup || !isCurrentGroup()) return anchor;

  const store = useGroupStore.getState();
  const currentEvents = store.chatByGroup[groupId]?.events || [];
  const reconciliations = catchup.events.map((event) =>
    reconcileCanonicalOutboxEvent(event, groupId),
  );
  const reconciledEvents = reconciliations.map((item) => item.event);
  const nextEvents =
    anchor && catchup.hasMoreHistory === null
      ? mergeForwardLedgerEvents(currentEvents, reconciledEvents, anchor)
      : mergeLedgerEvents(currentEvents, reconciledEvents, MAX_RECONCILED_EVENTS);

  store.setEvents(nextEvents, groupId);
  if (catchup.hasMoreHistory !== null) {
    store.setHasMoreHistory(catchup.hasMoreHistory, groupId);
  } else if (currentEvents.length + catchup.events.length > MAX_RECONCILED_EVENTS) {
    store.setHasMoreHistory(true, groupId);
  }
  for (const reconciliation of reconciliations) {
    const canonicalEventId = String(reconciliation.event.id || "").trim();
    if (reconciliation.clientId && canonicalEventId) {
      store.promoteStreamingEventsByPrefix(
        `local:${reconciliation.clientId}:`,
        canonicalEventId,
        groupId,
      );
    }
    completeCanonicalOutboxReconciliation(groupId, reconciliation);
  }

  const eventIds = nextEvents
    .filter((event) => event.kind === "chat.message")
    .map((event) => String(event.id || "").trim())
    .filter(Boolean);
  if (eventIds.length === 0) return catchup.cursor;
  const statuses = await api.fetchLedgerStatuses(groupId, eventIds);
  if (statuses.ok && isCurrentGroup()) {
    store.mergeEventStatuses(statuses.result.statuses || {}, groupId);
  }
  return catchup.cursor;
}
