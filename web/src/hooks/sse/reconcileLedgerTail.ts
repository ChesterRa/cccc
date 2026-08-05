import * as api from "../../services/api";
import { useGroupStore } from "../../stores";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "../../utils/chatOutboxReconciliation";
import { mergeLedgerEvents } from "../../utils/mergeLedgerEvents";

const MAX_RECONCILED_EVENTS = 800;
const RECONNECT_LEDGER_TAIL_LIMIT = 60;

export async function reconcileLedgerTail(
  groupId: string,
  isCurrentGroup: () => boolean,
): Promise<void> {
  const response = await api.fetchLedgerTail(groupId, RECONNECT_LEDGER_TAIL_LIMIT, {
    includeStatuses: false,
  });
  if (!response.ok || !isCurrentGroup()) return;

  const store = useGroupStore.getState();
  const currentEvents = store.chatByGroup[groupId]?.events || [];
  const reconciliations = response.result.events.map((event) =>
    reconcileCanonicalOutboxEvent(event, groupId),
  );
  const nextEvents = mergeLedgerEvents(
    currentEvents,
    reconciliations.map((item) => item.event),
    MAX_RECONCILED_EVENTS,
  );

  store.setEvents(nextEvents, groupId);
  store.setHasMoreHistory(!!response.result.has_more, groupId);
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
  if (eventIds.length === 0) return;
  const statuses = await api.fetchLedgerStatuses(groupId, eventIds);
  if (statuses.ok && isCurrentGroup()) {
    store.mergeEventStatuses(statuses.result.statuses || {}, groupId);
  }
}
