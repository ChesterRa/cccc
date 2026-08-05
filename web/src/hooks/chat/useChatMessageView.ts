import { useMemo } from "react";
import type { OutboxEntry } from "../../stores/chatOutboxStore";
import type { GroupChatBucket } from "../../stores/groupStoreCore";
import { getChatSession } from "../../stores/useUIStore";
import type { Actor, ChatMessageData, GroupDoc, LedgerEvent } from "../../types";
import { isFormalChatMessageEvent } from "../../utils/chatSend";
import { hasRenderableChatMessageContent } from "../../utils/ledgerEventHandlers";
import {
  shouldRestoreDetachedScrollSnapshot,
  shouldShowInConversation,
  toVisibleConversationEvent,
} from "./chatTabBasics";
import {
  buildUnfilteredLiveChatMessages,
  mergeVisibleChatMessages,
  sortChatMessages,
} from "./chatMessageOrdering";
import {
  collapseActorStreamingPlaceholders,
  dedupeStreamingEvents,
  dropOrphanQueuedPlaceholders,
} from "./chatStreamingProjection";

type ChatEmptyState = "ready" | "hydrating" | "business_empty";

export function useChatMessageView(input: {
  selectedGroupId: string;
  events: LedgerEvent[];
  streamingEvents: LedgerEvent[];
  outboxEntries: Pick<OutboxEntry, "localId" | "event">[];
  chatWindow: GroupChatBucket["chatWindow"];
  chatFilter: ReturnType<typeof getChatSession>["chatFilter"];
  scrollSnapshot: ReturnType<typeof getChatSession>["scrollSnapshot"];
  hasLoadedTail: boolean;
  hasMoreHistory: boolean;
  isLoadingHistory: boolean;
  isChatWindowLoading: boolean;
  groupDoc: GroupDoc | null;
  groupContext: unknown;
  groupSettings: unknown;
  actors: Actor[];
  needsActors: boolean;
}) {
  const inChatWindow =
    !!input.chatWindow &&
    String(input.chatWindow.groupId || "") === String(input.selectedGroupId || "");
  const viewKey =
    inChatWindow && input.chatWindow
      ? `${input.selectedGroupId}:window:${input.chatWindow.centerEventId}`
      : `${input.selectedGroupId}:live`;
  const orderState = useMemo(
    () => ({ viewKey, map: new Map<string, number>(), next: 0 }),
    [viewKey],
  );

  const liveWorkEvents = useMemo(() => {
    const canonical = input.events.filter((event) => event.kind === "chat.message");
    const streams = dedupeStreamingEvents(
      input.streamingEvents.filter((event) => event.kind === "chat.message"),
    );
    return dropOrphanQueuedPlaceholders(canonical, collapseActorStreamingPlaceholders(streams));
  }, [input.events, input.streamingEvents]);

  const unfilteredLiveChatMessages = useMemo(
    () => buildUnfilteredLiveChatMessages(input.events, input.outboxEntries, orderState),
    [input.events, input.outboxEntries, orderState],
  );

  const liveChatMessages = useMemo(() => {
    const canonical = input.events
      .filter(isFormalChatMessageEvent)
      .filter(shouldShowInConversation)
      .map(toVisibleConversationEvent);
    const canonicalClientIds = new Set(
      canonical
        .filter(hasRenderableChatMessageContent)
        .map((event) => {
          const data =
            event.data && typeof event.data === "object"
              ? (event.data as { client_id?: unknown })
              : null;
          return typeof data?.client_id === "string" ? data.client_id.trim() : "";
        })
        .filter(Boolean),
    );
    const pending = input.outboxEntries
      .filter((entry) => !canonicalClientIds.has(entry.localId))
      .map((entry) => entry.event);
    const ordered = sortChatMessages(
      mergeVisibleChatMessages(canonical, [], pending, orderState),
      new Map(),
    );
    if (input.chatFilter === "attention") {
      return ordered.filter(
        (event) => String((event.data as ChatMessageData)?.priority || "normal") === "attention",
      );
    }
    if (input.chatFilter === "task") {
      return ordered.filter((event) => !!(event.data as ChatMessageData)?.reply_required);
    }
    if (input.chatFilter === "user") {
      return ordered.filter((event) => {
        const data = event.data as ChatMessageData;
        if (String(data?.dst_group_id || "").trim()) return false;
        const to = Array.isArray(data?.to) ? data.to : [];
        return event.by === "user" || to.includes("user") || to.includes("@user");
      });
    }
    return ordered;
  }, [input.events, input.chatFilter, input.outboxEntries, orderState]);

  const chatMessages = useMemo(() => {
    if (!inChatWindow || !input.chatWindow) return liveChatMessages;
    return input.chatWindow.events
      .filter(isFormalChatMessageEvent)
      .filter(shouldShowInConversation)
      .map(toVisibleConversationEvent);
  }, [inChatWindow, input.chatWindow, liveChatMessages]);

  const hasAnyChatMessages = useMemo(
    () =>
      input.events.some(
        (event) => isFormalChatMessageEvent(event) && shouldShowInConversation(event),
      ) || input.outboxEntries.length > 0,
    [input.events, input.outboxEntries],
  );
  const restoreSnapshot =
    !inChatWindow && shouldRestoreDetachedScrollSnapshot(input.scrollSnapshot);
  const effectiveIsLoadingHistory = inChatWindow
    ? input.isChatWindowLoading
    : input.isLoadingHistory;
  const effectiveHasMoreHistory = !input.selectedGroupId
    ? false
    : inChatWindow
      ? false
      : !input.hasLoadedTail || input.hasMoreHistory;
  const hydratedDoc =
    !!input.groupDoc &&
    input.groupDoc.group_id === input.selectedGroupId &&
    (Object.prototype.hasOwnProperty.call(input.groupDoc, "scopes") ||
      Object.prototype.hasOwnProperty.call(input.groupDoc, "active_scope_key"));
  const settledActors =
    !!input.selectedGroupId &&
    (input.actors.length > 0 || input.groupContext !== null || input.groupSettings !== null);
  const emptyState: ChatEmptyState =
    chatMessages.length > 0
      ? "ready"
      : !input.selectedGroupId
        ? "business_empty"
        : effectiveIsLoadingHistory ||
            effectiveHasMoreHistory ||
            !hydratedDoc ||
            (input.needsActors && !settledActors)
          ? "hydrating"
          : "business_empty";

  const centerEventId = inChatWindow ? input.chatWindow?.centerEventId : undefined;
  return {
    inChatWindow,
    chatViewKey: viewKey,
    liveWorkEvents,
    unfilteredLiveChatMessages,
    chatMessages,
    hasAnyChatMessages,
    chatInitialScrollAnchorId: restoreSnapshot ? input.scrollSnapshot!.anchorId : undefined,
    chatInitialScrollAnchorOffsetPx: restoreSnapshot
      ? Number(input.scrollSnapshot!.offsetPx || 0)
      : undefined,
    chatInitialScrollOffsetPx:
      restoreSnapshot && Number.isFinite(Number(input.scrollSnapshot!.scrollTop))
        ? Math.max(0, Number(input.scrollSnapshot!.scrollTop))
        : undefined,
    chatWindowProps:
      inChatWindow && input.chatWindow
        ? {
            centerEventId: input.chatWindow.centerEventId,
            hasMoreBefore: input.chatWindow.hasMoreBefore,
            hasMoreAfter: input.chatWindow.hasMoreAfter,
          }
        : null,
    chatInitialScrollTargetId: centerEventId,
    chatHighlightEventId: centerEventId,
    effectiveIsLoadingHistory,
    effectiveHasMoreHistory,
    chatEmptyState: emptyState,
  };
}
