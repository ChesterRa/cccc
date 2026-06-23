import type { Actor, ChatMessageData, GroupSettings, LedgerEvent, ReplyTarget } from "../types";

type ReplyComposerState = {
  destGroupId: string;
  toText: string;
  replyTarget: ReplyTarget;
};

export function isEphemeralMessageEventId(eventId: string): boolean {
  const rawId = String(eventId || "").trim();
  if (!rawId) return true;
  return (
    rawId.startsWith("stream:")
    || rawId.startsWith("pending:")
    || rawId.startsWith("local:")
    || rawId.startsWith("local_")
  );
}

export function getReplyEventId(event: LedgerEvent): string {
  if (!event || event.kind !== "chat.message") return "";

  const rawId = String(event.id || "").trim();
  const data = event.data && typeof event.data === "object"
    ? (event.data as ChatMessageData & { pending_event_id?: unknown })
    : null;
  const pendingEventId = data && typeof data.pending_event_id === "string"
    ? String(data.pending_event_id || "").trim()
    : "";

  if (pendingEventId) return pendingEventId;
  if (isEphemeralMessageEventId(rawId)) return "";
  return rawId;
}

export function buildReplyComposerState(
  event: LedgerEvent,
  selectedGroupId: string,
  actors: Actor[],
  groupSettings: GroupSettings | null | undefined
): ReplyComposerState | null {
  const replyEventId = getReplyEventId(event);
  if (!replyEventId) return null;

  const data = event.data && typeof event.data === "object" ? (event.data as ChatMessageData) : null;
  const quoteText = data && typeof data.quote_text === "string" ? String(data.quote_text) : "";
  const messageText = data && typeof data.text === "string" ? String(data.text) : "";
  const text = quoteText || messageText;
  const by = String(event.by || "").trim();
  const sourcePlatform = data && typeof data.source_platform === "string" ? String(data.source_platform || "").trim() : "";
  const isGroupBridgeMessage = by.startsWith("group_bridge:") || sourcePlatform === "group_bridge_session";
  const authorIsActor = by && by !== "user" && actors.some((actor) => String(actor.id || "") === by);
  const originalTo = Array.isArray(data?.to)
    ? data.to.map((token: string) => String(token || "").trim()).filter(Boolean)
    : [];
  const policy = groupSettings?.default_send_to || "foreman";
  const defaultTo =
    isGroupBridgeMessage
      ? []
      : authorIsActor
      ? [by]
      : originalTo.length > 0
        ? originalTo
        : policy === "foreman"
          ? ["@foreman"]
          : [];

  return {
    destGroupId: String(selectedGroupId || "").trim(),
    toText: defaultTo.join(", "),
    replyTarget: {
      eventId: replyEventId,
      by: String(event.by || "unknown"),
      text: text.slice(0, 100) + (text.length > 100 ? "..." : ""),
    },
  };
}
