import type { Actor, ChatMessageData, GroupSettings, LedgerEvent, ReplyTarget } from "../types";
import { isConnectMessage, isCrossInstanceInboundMessage } from "./crossInstanceMessages";
import { projectCrossGroupRecipients } from "./crossGroupRecipients";
import { CHET_NAME, CHET_PRINCIPAL, isChetMessage } from "./chetIdentity";

type ReplyComposerState = { destGroupId: string; toText: string; replyTarget: ReplyTarget };

export function isEphemeralMessageEventId(eventId: string): boolean {
  const rawId = String(eventId || "").trim();
  if (!rawId) return true;
  return (
    rawId.startsWith("stream:") ||
    rawId.startsWith("pending:") ||
    rawId.startsWith("local:") ||
    rawId.startsWith("local_")
  );
}

export function getReplyEventId(event: LedgerEvent): string {
  if (!event || event.kind !== "chat.message") return "";

  const rawId = String(event.id || "").trim();
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as ChatMessageData & { pending_event_id?: unknown })
      : null;
  const pendingEventId =
    data && typeof data.pending_event_id === "string"
      ? String(data.pending_event_id || "").trim()
      : "";

  if (
    event._retired_bridge ||
    (!isConnectMessage(data) &&
      (isCrossInstanceInboundMessage(event.by, data) ||
        data?.source_platform === "group_bridge_session"))
  )
    return "";
  if (pendingEventId) return pendingEventId;
  if (isEphemeralMessageEventId(rawId)) return "";
  return rawId;
}

export function buildReplyComposerState(
  event: LedgerEvent,
  selectedGroupId: string,
  actors: Actor[],
  groupSettings: GroupSettings | null | undefined,
  /** The selected Group's generation, which a message Chet wrote in it carries. */
  groupGeneration = "",
): ReplyComposerState | null {
  const replyEventId = getReplyEventId(event);
  if (!replyEventId) return null;

  const data =
    event.data && typeof event.data === "object" ? (event.data as ChatMessageData) : null;
  const quoteText = data && typeof data.quote_text === "string" ? String(data.quote_text) : "";
  const messageText = data && typeof data.text === "string" ? String(data.text) : "";
  const text = quoteText || messageText;
  const by = String(event.by || "").trim();
  // The daemon resolves a Connect reply from the local canonical event. Remote
  // IDs must never be interpreted in this instance's Group namespace.
  if (isConnectMessage(data)) {
    return {
      destGroupId: selectedGroupId,
      toText: "",
      replyTarget: {
        eventId: replyEventId,
        connectInstanceId: data?.src_instance_id || data?.dst_instance_id,
        by: data?.sender_title || by,
        text: text.slice(0, 100) + (text.length > 100 ? "..." : ""),
      },
    };
  }
  const authorIsActor =
    by && by !== "user" && actors.some((actor) => String(actor.id || "") === by);
  // Keep a departed sender as an explicit, unavailable recipient. Falling
  // back to the original audience could send a reply to someone else.
  const authorIsUnavailableActor =
    by && by !== "user" && by !== "system" && !by.startsWith("svc:") && !authorIsActor;
  const originalTo = Array.isArray(data?.to)
    ? data.to.map((token: string) => String(token || "").trim()).filter(Boolean)
    : [];
  const remoteDstGroupId =
    typeof data?.dst_group_id === "string" ? String(data.dst_group_id || "").trim() : "";
  const remoteDstTo = remoteDstGroupId ? projectCrossGroupRecipients(data) : [];
  const policy = groupSettings?.default_send_to || "foreman";
  // A reply to Chet names no recipient: the daemon returns it to Chet once it has
  // verified that Chet wrote the message in this Group.
  const defaultTo = remoteDstGroupId
    ? remoteDstTo
    : by === CHET_PRINCIPAL
      ? []
      : authorIsActor
        ? [by]
        : authorIsUnavailableActor
          ? [by]
          : originalTo.length > 0
            ? originalTo
            : policy === "foreman"
              ? ["@foreman"]
              : [];
  // Named Chet only where the message row names it; otherwise the bare principal shows.
  const replyBy = isChetMessage(event, groupGeneration)
    ? CHET_NAME
    : by === "user" && defaultTo.length > 0
      ? defaultTo.join(", ")
      : String(event.by || "unknown");

  return {
    destGroupId: remoteDstGroupId || String(selectedGroupId || "").trim(),
    toText: defaultTo.join(", "),
    replyTarget: {
      eventId: replyEventId,
      by: replyBy,
      text: text.slice(0, 100) + (text.length > 100 ? "..." : ""),
      ...(by === CHET_PRINCIPAL ? { toChet: true } : {}),
      ...(remoteDstGroupId ? { remoteDstGroupId, remoteDstTo: defaultTo } : {}),
    },
  };
}
