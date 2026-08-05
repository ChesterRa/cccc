import type { ChatMessageData, LedgerEvent } from "../../types";
import type { OutboxEntry } from "../../stores/chatOutboxStore";
import { isFormalChatMessageEvent } from "../../utils/chatSend";
import { hasRenderableChatMessageContent } from "../../utils/ledgerEventHandlers";
import { shouldShowInConversation, toVisibleConversationEvent } from "./chatTabBasics";
import { getReplySlotKey } from "./chatReplySlots";
import {
  getCanonicalStreamingSupersededStreamIds,
  hasOnlyQueuedActivities,
  hasRichActivities,
  isPlaceholderLikeStreamingEvent,
} from "./chatStreamingProjection";

export function sortChatMessages(
  messages: LedgerEvent[],
  replySlotTsByKey: Map<string, string>,
): LedgerEvent[] {
  return messages
    .map((event, index) => {
      const slotKey = getReplySlotKey(event);
      const slotTs = slotKey ? String(replySlotTsByKey.get(slotKey) || "").trim() : "";
      const eventTs = String(event.ts || "").trim();
      return { event, index, hasReplySlot: slotKey.length > 0, sortTs: slotTs || eventTs, eventTs };
    })
    .sort((a, b) => {
      if (a.sortTs && b.sortTs && a.sortTs !== b.sortTs) return a.sortTs.localeCompare(b.sortTs);
      if (a.sortTs && !b.sortTs) return -1;
      if (!a.sortTs && b.sortTs) return 1;
      if (a.sortTs && b.sortTs && a.sortTs === b.sortTs && a.hasReplySlot !== b.hasReplySlot) {
        return a.hasReplySlot ? 1 : -1;
      }
      if (a.eventTs && b.eventTs && a.eventTs !== b.eventTs)
        return a.eventTs.localeCompare(b.eventTs);
      return a.index - b.index;
    })
    .map((item) => item.event);
}

function getLogicalMessageOrderKey(event: LedgerEvent): string {
  if (String(event.kind || "").trim() !== "chat.message") {
    return `event:${String(event.id || "").trim() || String(event.ts || "").trim()}`;
  }
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as ChatMessageData & {
          client_id?: unknown;
          pending_event_id?: unknown;
          reply_to?: unknown;
          stream_id?: unknown;
        })
      : undefined;
  const clientId = typeof data?.client_id === "string" ? data.client_id.trim() : "";
  if (clientId) return `client:${clientId}`;

  const actorId = String(event.by || "").trim();
  const replyAnchor =
    typeof data?.pending_event_id === "string" && data.pending_event_id.trim()
      ? data.pending_event_id.trim()
      : typeof data?.reply_to === "string" && data.reply_to.trim()
        ? data.reply_to.trim()
        : "";
  const streamId = typeof data?.stream_id === "string" ? data.stream_id.trim() : "";
  if (
    actorId &&
    actorId !== "user" &&
    replyAnchor &&
    (event._streaming || !hasRenderableChatMessageContent(event) || streamId)
  ) {
    return `reply:${actorId}:${replyAnchor}`;
  }

  if (streamId) return `stream:${streamId}`;

  const eventId = String(event.id || "").trim();
  if (eventId) return `event:${eventId}`;
  return `fallback:${actorId}:${String(event.ts || "").trim()}`;
}

function getLogicalMessageReplacementKey(event: LedgerEvent): string {
  if (String(event.kind || "").trim() !== "chat.message") {
    return `event:${String(event.id || "").trim() || String(event.ts || "").trim()}`;
  }
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as ChatMessageData & {
          client_id?: unknown;
          pending_event_id?: unknown;
          reply_to?: unknown;
          stream_id?: unknown;
        })
      : undefined;
  const clientId = typeof data?.client_id === "string" ? data.client_id.trim() : "";
  if (clientId) return `client:${clientId}`;

  const actorId = String(event.by || "").trim();
  const replyAnchor =
    typeof data?.pending_event_id === "string" && data.pending_event_id.trim()
      ? data.pending_event_id.trim()
      : typeof data?.reply_to === "string" && data.reply_to.trim()
        ? data.reply_to.trim()
        : "";
  const streamId = typeof data?.stream_id === "string" ? data.stream_id.trim() : "";
  const placeholderLike = isPlaceholderLikeStreamingEvent(
    (data || {}) as ChatMessageData & { pending_placeholder?: unknown; stream_id?: unknown },
  );
  if (actorId && actorId !== "user" && replyAnchor) {
    if (streamId && !placeholderLike) {
      return `stream:${streamId}`;
    }
    if (placeholderLike || !hasRenderableChatMessageContent(event)) {
      return `reply:${actorId}:${replyAnchor}`;
    }
  }

  if (streamId) return `stream:${streamId}`;

  const eventId = String(event.id || "").trim();
  if (eventId) return `event:${eventId}`;

  return `fallback:${String(event.by || "").trim()}:${String(event.ts || "").trim()}`;
}

function getLogicalMessagePriority(event: LedgerEvent): number {
  const isStreaming = !!event._streaming;
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as { _optimistic?: unknown })
      : undefined;
  const isOptimistic = Boolean(data?._optimistic);
  if (!isStreaming && !isOptimistic) return 3;
  if (isOptimistic) return 2;
  return 1;
}

function shouldReplaceLogicalMessage(existing: LedgerEvent, incoming: LedgerEvent): boolean {
  const existingRenderable = hasRenderableChatMessageContent(existing);
  const incomingRenderable = hasRenderableChatMessageContent(incoming);
  if (incomingRenderable !== existingRenderable) {
    return incomingRenderable;
  }

  if (
    !existingRenderable &&
    !incomingRenderable &&
    !!existing._streaming !== !!incoming._streaming
  ) {
    return !!incoming._streaming;
  }

  return getLogicalMessagePriority(incoming) >= getLogicalMessagePriority(existing);
}

export function mergeLogicalMessagesWithStableOrder(
  candidates: LedgerEvent[],
  orderState: { map: Map<string, number>; next: number },
): LedgerEvent[] {
  const mergedByReplacementKey = new Map<
    string,
    { orderKey: string; event: LedgerEvent; index: number }
  >();
  candidates.forEach((event, index) => {
    const orderKey = getLogicalMessageOrderKey(event);
    if (!orderState.map.has(orderKey)) {
      orderState.map.set(orderKey, orderState.next);
      orderState.next += 1;
    }
    const replacementKey = getLogicalMessageReplacementKey(event);
    const existing = mergedByReplacementKey.get(replacementKey);
    if (!existing || shouldReplaceLogicalMessage(existing.event, event)) {
      mergedByReplacementKey.set(replacementKey, { orderKey, event, index });
    }
  });

  return Array.from(mergedByReplacementKey.values())
    .sort((a, b) => {
      const ao = orderState.map.get(a.orderKey) ?? Number.MAX_SAFE_INTEGER;
      const bo = orderState.map.get(b.orderKey) ?? Number.MAX_SAFE_INTEGER;
      if (ao !== bo) return ao - bo;
      const ats = String(a.event.ts || "").trim();
      const bts = String(b.event.ts || "").trim();
      if (ats && bts && ats !== bts) return ats.localeCompare(bts);
      return a.index - b.index;
    })
    .map((item) => item.event);
}

export function mergeVisibleChatMessages(
  canonicalEvents: LedgerEvent[],
  streamingEvents: LedgerEvent[],
  pendingEvents: LedgerEvent[],
  orderState: { map: Map<string, number>; next: number },
): LedgerEvent[] {
  const canonicalStreamIds = getCanonicalStreamingSupersededStreamIds(canonicalEvents);
  const canonicalReplySlots = new Set(
    canonicalEvents
      .filter((ev: LedgerEvent) => hasRenderableChatMessageContent(ev))
      .map((ev: LedgerEvent) => getReplySlotKey(ev))
      .filter((key: string) => key.length > 0),
  );
  const renderableStreamingReplySlots = new Set(
    streamingEvents
      .filter((ev: LedgerEvent) => hasRenderableChatMessageContent(ev))
      .map((ev: LedgerEvent) => getReplySlotKey(ev))
      .filter((key: string) => key.length > 0),
  );
  const liveStreaming = streamingEvents.filter((ev: LedgerEvent) => {
    const data =
      ev.data && typeof ev.data === "object"
        ? (ev.data as { stream_id?: unknown; pending_placeholder?: unknown; activities?: unknown })
        : null;
    const streamId = data && typeof data.stream_id === "string" ? data.stream_id.trim() : "";
    const slotKey = getReplySlotKey(ev);
    const renderable = hasRenderableChatMessageContent(ev);
    if (streamId && canonicalStreamIds.has(streamId)) return false;
    const hasRichActivityTimeline = hasRichActivities(data?.activities);
    // Backup: drop empty streaming events whose reply slot is covered by a canonical event,
    // but keep non-queued activity bubbles until the activity itself completes.
    if (!renderable) {
      if (slotKey && canonicalReplySlots.has(slotKey)) return hasRichActivityTimeline;
      if (slotKey && renderableStreamingReplySlots.has(slotKey)) {
        const placeholderLike = isPlaceholderLikeStreamingEvent(
          (data || {}) as ChatMessageData & { pending_placeholder?: unknown; stream_id?: unknown },
        );
        if (
          !hasRichActivityTimeline &&
          (placeholderLike || hasOnlyQueuedActivities(data?.activities))
        )
          return false;
      }
    }
    return true;
  });

  return mergeLogicalMessagesWithStableOrder(
    [...canonicalEvents, ...pendingEvents, ...liveStreaming],
    orderState,
  );
}

export type LogicalMessageOrderState = { map: Map<string, number>; next: number };

export function buildUnfilteredLiveChatMessages(
  events: LedgerEvent[],
  outboxEntries: Pick<OutboxEntry, "localId" | "event">[],
  orderState: LogicalMessageOrderState,
): LedgerEvent[] {
  const all = events
    .filter(isFormalChatMessageEvent)
    .filter(shouldShowInConversation)
    .map(toVisibleConversationEvent);
  const renderableCanonicalClientIds = new Set(
    all
      .filter((ev: LedgerEvent) => hasRenderableChatMessageContent(ev))
      .map((ev: LedgerEvent) => {
        const data =
          ev.data && typeof ev.data === "object" ? (ev.data as { client_id?: unknown }) : null;
        return data && typeof data.client_id === "string" ? data.client_id.trim() : "";
      })
      .filter((clientId: string) => clientId.length > 0),
  );
  const pendingEvents = outboxEntries
    .filter((entry) => !renderableCanonicalClientIds.has(entry.localId))
    .map((entry) => entry.event);

  return sortChatMessages(mergeVisibleChatMessages(all, [], pendingEvents, orderState), new Map());
}
