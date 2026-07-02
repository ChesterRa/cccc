import type { LedgerEvent } from "../types";

const CROSS_GROUP_RECEIPT_KIND = "chat.cross_group_receipt";

function mergeEventWithExistingStatus(incoming: LedgerEvent, existing?: LedgerEvent): LedgerEvent {
  if (!existing) return incoming;
  return {
    ...incoming,
    _read_status: incoming._read_status ?? existing._read_status,
    _ack_status: incoming._ack_status ?? existing._ack_status,
    _obligation_status: incoming._obligation_status ?? existing._obligation_status,
  };
}

function stringDataValue(event: LedgerEvent, key: string): string {
  const data = event?.data;
  if (!data || typeof data !== "object") return "";
  const value = (data as Record<string, unknown>)[key];
  return typeof value === "string" ? value.trim() : "";
}

export function projectCrossGroupReceipts(events: LedgerEvent[]): LedgerEvent[] {
  const receipts = events.filter((event) => String(event?.kind || "") === CROSS_GROUP_RECEIPT_KIND);
  if (receipts.length === 0) return events;

  const anchorsBySourceId = new Map<string, { dstEventId: string; remoteEventId: string }>();
  for (const receipt of receipts) {
    const sourceEventId = stringDataValue(receipt, "source_event_id");
    if (!sourceEventId) continue;
    const dstEventId = stringDataValue(receipt, "dst_event_id");
    const remoteEventId = stringDataValue(receipt, "remote_event_id");
    if (!dstEventId && !remoteEventId) continue;
    const existing = anchorsBySourceId.get(sourceEventId);
    anchorsBySourceId.set(sourceEventId, {
      dstEventId: dstEventId || existing?.dstEventId || "",
      remoteEventId: remoteEventId || existing?.remoteEventId || "",
    });
  }
  if (anchorsBySourceId.size === 0) {
    return events.filter((event) => String(event?.kind || "") !== CROSS_GROUP_RECEIPT_KIND);
  }

  return events
    .filter((event) => String(event?.kind || "") !== CROSS_GROUP_RECEIPT_KIND)
    .map((event) => {
      const eventId = String(event?.id || "").trim();
      const anchor = eventId ? anchorsBySourceId.get(eventId) : undefined;
      if (!anchor || String(event?.kind || "") !== "chat.message") return event;
      const data = event.data && typeof event.data === "object" ? event.data : {};
      return {
        ...event,
        data: {
          ...data,
          ...(anchor.dstEventId ? { dst_event_id: anchor.dstEventId } : {}),
          ...(anchor.remoteEventId ? { remote_event_id: anchor.remoteEventId } : {}),
        },
      };
    });
}

export function mergeLedgerEvents(existing: LedgerEvent[], incoming: LedgerEvent[], maxEvents: number): LedgerEvent[] {
  const nextIncoming = Array.isArray(incoming) ? incoming.filter(Boolean) : [];
  if (nextIncoming.length === 0) {
    const nextExisting = projectCrossGroupReceipts(Array.isArray(existing) ? existing.filter(Boolean) : []);
    return nextExisting.length > maxEvents ? nextExisting.slice(nextExisting.length - maxEvents) : nextExisting;
  }
  const existingById = new Map(
    (Array.isArray(existing) ? existing : [])
      .map((event) => [String(event?.id || "").trim(), event] as const)
      .filter(([eventId]) => eventId.length > 0)
  );
  const hydratedIncoming = nextIncoming.map((event) => {
    const eventId = String(event?.id || "").trim();
    return eventId ? mergeEventWithExistingStatus(event, existingById.get(eventId)) : event;
  });

  const incomingIds = new Set(
    hydratedIncoming
      .map((event) => String(event?.id || "").trim())
      .filter((eventId) => eventId.length > 0)
  );

  const localOnlyExisting = (Array.isArray(existing) ? existing : []).filter((event) => {
    if (!event) return false;
    const eventId = String(event.id || "").trim();
    return !eventId || !incomingIds.has(eventId);
  });

  const merged = projectCrossGroupReceipts([...hydratedIncoming, ...localOnlyExisting]
    .map((event, index) => ({
      event,
      index,
      ts: Date.parse(String(event.ts || "")),
    }))
    .sort((left, right) => {
      const leftValid = Number.isFinite(left.ts);
      const rightValid = Number.isFinite(right.ts);
      if (leftValid && rightValid && left.ts !== right.ts) return left.ts - right.ts;
      if (leftValid !== rightValid) return leftValid ? -1 : 1;
      return left.index - right.index;
    })
    .map((entry) => entry.event));

  return merged.length > maxEvents ? merged.slice(merged.length - maxEvents) : merged;
}
