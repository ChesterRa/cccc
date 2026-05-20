import type { LedgerEvent } from "../types";
import { getChatTailMutationSnapshot, getChatTailSnapshot, shouldAutoFollowOnTailAppend, shouldAutoFollowOnTailMutation } from "../utils/chatAutoFollow";
import { hasRenderableChatMessageContent } from "../utils/ledgerEventHandlers";

const VIRTUALIZATION_THRESHOLD = 80;

function hasOnlyQueuedActivities(value: unknown): boolean {
  const activities = Array.isArray(value) ? value : [];
  return activities.length === 0 || activities.every((item) => {
    if (!item || typeof item !== "object") return true;
    const kind = String((item as { kind?: unknown }).kind || "").trim();
    const summary = String((item as { summary?: unknown }).summary || "").trim();
    return kind === "queued" && summary === "queued";
  });
}

function isPlaceholderLikeMessageData(data: {
  pending_placeholder?: unknown;
  stream_id?: unknown;
  stream_phase?: unknown;
  text?: unknown;
  activities?: unknown;
}): boolean {
  if (data.pending_placeholder) return true;

  const streamPhase = String(data.stream_phase || "").trim().toLowerCase();
  if (streamPhase === "commentary" || streamPhase === "final_answer") return false;

  const text = typeof data.text === "string" ? data.text.trim() : "";
  if (text) return false;
  if (!hasOnlyQueuedActivities(data.activities)) return false;

  const streamId = typeof data.stream_id === "string" ? data.stream_id.trim() : "";
  return streamId.startsWith("local:") || streamId.startsWith("pending:");
}

export function getAutoFollowTrigger(input: {
  previousTailSnapshot: ReturnType<typeof getChatTailSnapshot>;
  nextTailSnapshot: ReturnType<typeof getChatTailSnapshot>;
  previousTailMutationSnapshot: ReturnType<typeof getChatTailMutationSnapshot>;
  nextTailMutationSnapshot: ReturnType<typeof getChatTailMutationSnapshot>;
}): "append" | "mutation" | null {
  if (shouldAutoFollowOnTailAppend(input.previousTailSnapshot, input.nextTailSnapshot)) {
    return "append";
  }
  if (shouldAutoFollowOnTailMutation(input.previousTailMutationSnapshot, input.nextTailMutationSnapshot)) {
    return "mutation";
  }
  return null;
}

export function shouldDetachChatFollowOnScroll(input: {
  followMode: "follow" | "detached";
  previousTop: number;
  currentTop: number;
  atBottom: boolean;
  isContainerResizing: boolean;
  topLoadThresholdPx?: number;
}): boolean {
  if (input.followMode !== "follow") return false;
  if (input.isContainerResizing || input.atBottom) return false;

  const topLoadThresholdPx = Math.max(0, Number(input.topLoadThresholdPx) || 0);
  if (input.currentTop <= topLoadThresholdPx) return true;
  return input.currentTop < input.previousTop;
}

export function shouldNotifyScrollChange(input: {
  wasAtBottom: boolean;
  atBottom: boolean;
  showScrollButton: boolean;
  chatUnreadCount: number;
}): boolean {
  if (input.atBottom && (input.showScrollButton || input.chatUnreadCount > 0)) return true;
  return input.atBottom !== input.wasAtBottom;
}

export function shouldAutoScrollToBottom(input: {
  followMode: "follow" | "detached";
  isAtBottom: boolean;
  forceStickToBottom: boolean;
}): boolean {
  if (input.forceStickToBottom) return true;
  return input.followMode === "follow" && input.isAtBottom;
}

export function shouldRunScheduledBottomScroll(input: {
  followMode: "follow" | "detached";
  isAtBottom: boolean;
  forceStickToBottom: boolean;
  explicitForce: boolean;
}): boolean {
  if (input.explicitForce) return true;
  return shouldAutoScrollToBottom({
    followMode: input.followMode,
    isAtBottom: input.isAtBottom,
    forceStickToBottom: input.forceStickToBottom,
  });
}

export function getStableMessageKey(message: LedgerEvent | undefined, index: number): string | number {
  if (message?.kind === "chat.message" && message.data && typeof message.data === "object") {
    const eventId = typeof message.id === "string" ? String(message.id || "").trim() : "";
    if (eventId && (message._streaming || eventId.startsWith("local:") || eventId.startsWith("stream:"))) {
      return `message-event:${eventId}`;
    }
    const streamId = typeof (message.data as { stream_id?: unknown }).stream_id === "string"
      ? String((message.data as { stream_id?: string }).stream_id || "").trim()
      : "";
    const isPlaceholderLike = isPlaceholderLikeMessageData(message.data as {
      pending_placeholder?: unknown;
      stream_id?: unknown;
      stream_phase?: unknown;
      text?: unknown;
      activities?: unknown;
    });
    // Daemon-emitted canonical replies can carry both stream_id and
    // pending_event_id. Renderable stream-backed rows must key by stream_id,
    // otherwise TanStack reuses DOM/measurement state across distinct messages
    // from the same reply slot and produces gaps/overlap.
    if (streamId && !isPlaceholderLike) {
      return `stream:${streamId}`;
    }
    const pendingEventId = typeof (message.data as { pending_event_id?: unknown }).pending_event_id === "string"
      ? String((message.data as { pending_event_id?: string }).pending_event_id || "").trim()
      : "";
    const actorId = typeof message.by === "string" ? String(message.by || "").trim() : "";
    if (pendingEventId && actorId && (!hasRenderableChatMessageContent(message) || isPlaceholderLike)) {
      return `pending:${actorId}:${pendingEventId}`;
    }
    if (streamId) return `stream:${streamId}`;
    const clientId = typeof (message.data as { client_id?: unknown }).client_id === "string"
      ? String((message.data as { client_id?: string }).client_id || "").trim()
      : "";
    if (clientId) return `client:${clientId}`;
  }
  const eventId = typeof message?.id === "string" ? String(message.id || "").trim() : "";
  return eventId || index;
}

export function shouldUseVirtualizedMessageList(messageCount: number): boolean {
  return Math.max(0, Number(messageCount) || 0) >= VIRTUALIZATION_THRESHOLD;
}
