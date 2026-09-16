import type { ConnectDeliveryStatus, LedgerEvent, ObligationStatus } from "../types";

const CROSS_GROUP_RECEIPT_KIND = "chat.cross_group_receipt";

export function mergeReadStatus(
  incoming?: Record<string, boolean>,
  existing?: Record<string, boolean>,
): Record<string, boolean> | undefined {
  if (!incoming || !existing) return incoming ?? existing;
  // Keep the authoritative recipient set, including generation-based removals.
  return Object.fromEntries(
    Object.entries(incoming).map(([actor, read]) => [actor, read || existing[actor] === true]),
  );
}

export function mergeObligationStatus(
  incoming?: Record<string, ObligationStatus>,
  existing?: Record<string, ObligationStatus>,
): Record<string, ObligationStatus> | undefined {
  if (!incoming || !existing) return incoming ?? existing;
  return Object.fromEntries(
    Object.entries(incoming).map(([actor, status]) => {
      const previous = existing[actor];
      // A late pending snapshot cannot reopen a completed obligation. A terminal
      // snapshot still owns append-order resolution; delivery attempts may retry.
      return [
        actor,
        previous && !status.replied && !status.cancelled && (previous.replied || previous.cancelled)
          ? { ...status, replied: previous.replied, cancelled: previous.cancelled }
          : status,
      ];
    }),
  );
}

export function mergeConnectDelivery(
  incoming?: ConnectDeliveryStatus,
  existing?: ConnectDeliveryStatus,
): ConnectDeliveryStatus | undefined {
  // A late snapshot taken before the final receipt cannot undo that receipt.
  return existing && existing.state !== "queued" && incoming?.state === "queued"
    ? existing
    : (incoming ?? existing);
}

export function mergeEventWithExistingStatus(
  incoming: LedgerEvent,
  existing?: LedgerEvent,
): LedgerEvent {
  if (!existing) return incoming;
  return {
    ...incoming,
    _retired_bridge: incoming._retired_bridge || existing._retired_bridge,
    _read_status: mergeReadStatus(incoming._read_status, existing._read_status),
    _obligation_status: mergeObligationStatus(
      incoming._obligation_status,
      existing._obligation_status,
    ),
    _connect_delivery: mergeConnectDelivery(incoming._connect_delivery, existing._connect_delivery),
    _connect_cancellation: mergeConnectDelivery(
      incoming._connect_cancellation,
      existing._connect_cancellation,
    ),
  };
}

function stringDataValue(event: LedgerEvent, key: string): string {
  const data = event?.data;
  if (!data || typeof data !== "object") return "";
  const value = (data as Record<string, unknown>)[key];
  return typeof value === "string" ? value.trim() : "";
}

export function projectCrossGroupReceipts(events: LedgerEvent[]): LedgerEvent[] {
  const cancellations = new Map<string, ConnectDeliveryStatus>();
  for (const event of events) {
    const data = event.data as Record<string, unknown> | undefined;
    if (event.kind !== "chat.reply_request.cancelled" || !data?.connect_cancel) continue;
    const source = stringDataValue(event, "source_event_id");
    if (source)
      cancellations.set(source, { state: data.connect_cancel_sha256 ? "sent" : "queued" });
  }
  const receipts = events.filter((event) => String(event?.kind || "") === CROSS_GROUP_RECEIPT_KIND);
  if (receipts.length === 0 && cancellations.size === 0) return events;

  const retired = new Set<string>();
  const anchorsBySourceId = new Map<
    string,
    { dstEventId: string; remoteEventId: string; delivery?: ConnectDeliveryStatus }
  >();
  for (const receipt of receipts) {
    const sourceEventId = stringDataValue(receipt, "source_event_id");
    if (!sourceEventId) continue;
    const dstEventId = stringDataValue(receipt, "dst_event_id");
    const remoteEventId = stringDataValue(receipt, "remote_event_id");
    const state = stringDataValue(receipt, "status");
    if (
      stringDataValue(receipt, "transport") !== "connect" &&
      (remoteEventId || (receipt.data as Record<string, unknown>)?.group_bridge_retired === true)
    )
      retired.add(sourceEventId);
    const delivery: ConnectDeliveryStatus | undefined =
      stringDataValue(receipt, "transport") === "connect" &&
      (state === "sent" || state === "failed" || state === "unconfirmed")
        ? {
            state,
            error: stringDataValue(receipt, "error"),
            remote_event_id: remoteEventId || null,
          }
        : undefined;
    if (stringDataValue(receipt, "action") === "cancel") {
      const originalId = stringDataValue(receipt, "original_event_id");
      if (originalId && delivery) cancellations.set(originalId, delivery);
      continue;
    }
    if (!dstEventId && !remoteEventId && !delivery) continue;
    const existing = anchorsBySourceId.get(sourceEventId);
    anchorsBySourceId.set(sourceEventId, {
      dstEventId: dstEventId || existing?.dstEventId || "",
      remoteEventId: remoteEventId || existing?.remoteEventId || "",
      delivery: delivery ?? existing?.delivery,
    });
  }
  if (anchorsBySourceId.size === 0 && cancellations.size === 0 && retired.size === 0) {
    return events.filter((event) => String(event?.kind || "") !== CROSS_GROUP_RECEIPT_KIND);
  }

  return events
    .filter((event) => String(event?.kind || "") !== CROSS_GROUP_RECEIPT_KIND)
    .map((event) => {
      const eventId = String(event?.id || "").trim();
      const anchor = eventId ? anchorsBySourceId.get(eventId) : undefined;
      const cancellation = cancellations.get(eventId);
      if ((!anchor && !cancellation && !retired.has(eventId)) || event.kind !== "chat.message")
        return event;
      const data = event.data && typeof event.data === "object" ? event.data : {};
      return {
        ...event,
        _retired_bridge: retired.has(eventId) || event._retired_bridge,
        _connect_delivery: mergeConnectDelivery(anchor?.delivery, event._connect_delivery),
        _connect_cancellation: mergeConnectDelivery(cancellation, event._connect_cancellation),
        data: {
          ...data,
          ...(anchor?.dstEventId ? { dst_event_id: anchor.dstEventId } : {}),
          ...(anchor?.remoteEventId ? { remote_event_id: anchor.remoteEventId } : {}),
        },
      };
    });
}

export function mergeLedgerEvents(
  existing: LedgerEvent[],
  incoming: LedgerEvent[],
  maxEvents: number,
): LedgerEvent[] {
  const nextIncoming = Array.isArray(incoming) ? incoming.filter(Boolean) : [];
  if (nextIncoming.length === 0) {
    const nextExisting = projectCrossGroupReceipts(
      Array.isArray(existing) ? existing.filter(Boolean) : [],
    );
    return nextExisting.length > maxEvents
      ? nextExisting.slice(nextExisting.length - maxEvents)
      : nextExisting;
  }
  const existingById = new Map(
    (Array.isArray(existing) ? existing : [])
      .map((event) => [String(event?.id || "").trim(), event] as const)
      .filter(([eventId]) => eventId.length > 0),
  );
  const hydratedIncoming = nextIncoming.map((event) => {
    const eventId = String(event?.id || "").trim();
    return eventId ? mergeEventWithExistingStatus(event, existingById.get(eventId)) : event;
  });

  const incomingIds = new Set(
    hydratedIncoming
      .map((event) => String(event?.id || "").trim())
      .filter((eventId) => eventId.length > 0),
  );

  const localOnlyExisting = (Array.isArray(existing) ? existing : []).filter((event) => {
    if (!event) return false;
    const eventId = String(event.id || "").trim();
    return !eventId || !incomingIds.has(eventId);
  });

  const merged = projectCrossGroupReceipts(
    [...hydratedIncoming, ...localOnlyExisting]
      .map((event, index) => ({ event, index, ts: Date.parse(String(event.ts || "")) }))
      .sort((left, right) => {
        const leftValid = Number.isFinite(left.ts);
        const rightValid = Number.isFinite(right.ts);
        if (leftValid && rightValid && left.ts !== right.ts) return left.ts - right.ts;
        if (leftValid !== rightValid) return leftValid ? -1 : 1;
        return left.index - right.index;
      })
      .map((entry) => entry.event),
  );

  return merged.length > maxEvents ? merged.slice(merged.length - maxEvents) : merged;
}
