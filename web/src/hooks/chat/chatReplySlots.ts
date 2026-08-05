import type { ChatMessageData, LedgerEvent } from "../../types";

export function getReplySlotKey(event: LedgerEvent): string {
  if (String(event.kind || "").trim() !== "chat.message") return "";
  const actorId = String(event.by || "").trim();
  if (!actorId || actorId === "user") return "";
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as ChatMessageData & { pending_event_id?: unknown; reply_to?: unknown })
      : undefined;
  const replyAnchor =
    typeof data?.pending_event_id === "string" && data.pending_event_id.trim()
      ? data.pending_event_id.trim()
      : typeof data?.reply_to === "string" && data.reply_to.trim()
        ? data.reply_to.trim()
        : "";
  if (!replyAnchor) return "";
  return `${actorId}:${replyAnchor}`;
}

function getReplyAnchorId(event: LedgerEvent): string {
  if (String(event.kind || "").trim() !== "chat.message") return "";
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as ChatMessageData & { pending_event_id?: unknown; reply_to?: unknown })
      : undefined;
  if (typeof data?.pending_event_id === "string" && data.pending_event_id.trim()) {
    return data.pending_event_id.trim();
  }
  if (typeof data?.reply_to === "string" && data.reply_to.trim()) {
    return data.reply_to.trim();
  }
  return "";
}

export function buildReplySlotTsMap(streamingEvents: LedgerEvent[]): Map<string, string> {
  const slotTsByKey = new Map<string, string>();
  for (const event of streamingEvents) {
    const slotKey = getReplySlotKey(event);
    if (!slotKey) continue;
    const ts = String(event.ts || "").trim();
    if (!ts) continue;
    const prev = slotTsByKey.get(slotKey) || "";
    if (!prev || ts < prev) {
      slotTsByKey.set(slotKey, ts);
    }
  }
  return slotTsByKey;
}

export function buildReplyAnchorTsMap(
  messages: LedgerEvent[],
  streamingEvents: LedgerEvent[],
): Map<string, string> {
  const slotTsByKey = buildReplySlotTsMap(streamingEvents);
  const anchorTsById = new Map<string, string>();

  for (const event of messages) {
    if (String(event.kind || "").trim() !== "chat.message") continue;
    const ts = String(event.ts || "").trim();
    if (!ts) continue;
    const eventId = String(event.id || "").trim();
    if (eventId) {
      const prev = anchorTsById.get(eventId) || "";
      if (!prev || ts < prev) anchorTsById.set(eventId, ts);
    }
    const data =
      event.data && typeof event.data === "object"
        ? (event.data as ChatMessageData & { client_id?: unknown })
        : undefined;
    const clientId = typeof data?.client_id === "string" ? data.client_id.trim() : "";
    if (clientId) {
      const prev = anchorTsById.get(clientId) || "";
      if (!prev || ts < prev) anchorTsById.set(clientId, ts);
    }
  }

  for (const event of [...messages, ...streamingEvents]) {
    const slotKey = getReplySlotKey(event);
    if (!slotKey) continue;
    const anchorId = getReplyAnchorId(event);
    if (!anchorId) continue;
    const anchorTs = String(anchorTsById.get(anchorId) || "").trim();
    if (!anchorTs) continue;
    slotTsByKey.set(slotKey, anchorTs);
  }

  return slotTsByKey;
}
