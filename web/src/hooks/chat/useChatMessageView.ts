import { useMemo } from "react";
import type { OutboxEntry } from "../../stores/chatOutboxStore";
import type { GroupChatBucket } from "../../stores/groupStoreCore";
import { getChatSession } from "../../stores/useUIStore";
import type { Actor, GroupDoc, LedgerEvent } from "../../types";
import { isFormalChatMessageEvent } from "../../utils/chatSend";
import { hasRenderableChatMessageContent } from "../../utils/ledgerEventHandlers";
import {
  shouldRestoreScrollSnapshot,
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
import { resolveChatListHistoryState } from "./chatListHistoryState";
import { resolveInitialChatScroll } from "./chatScrollFocus";
import { matchesChatFilters } from "./chatMessageFilters";
import { useChatSenderHistory } from "./useChatSenderHistory";
import { usePendingPages } from "../../features/pendingReplies/usePendingPages";
import { collectTaskConversation } from "../../features/taskView/taskConversationModel";
import { useTaskConversation } from "../../features/taskView/useTaskConversation";
import { usePinnedMessages } from "../../features/pins/usePinnedMessages";

const EMPTY_SENDERS: readonly string[] = [];
const EMPTY_IDS: readonly string[] = [];

type ChatEmptyState = "ready" | "hydrating" | "business_empty" | "filtered_empty";

export function useChatMessageView(input: {
  selectedGroupId: string;
  liveJump: { groupId: string; eventId: string; requestId: number } | null;
  events: LedgerEvent[];
  streamingEvents: LedgerEvent[];
  outboxEntries: Pick<OutboxEntry, "localId" | "event">[];
  chatWindow: GroupChatBucket["chatWindow"];
  chatFilter: ReturnType<typeof getChatSession>["chatFilter"];
  senders?: readonly string[];
  /** The task whose conversation replaces the full one; empty shows the full conversation. */
  taskId?: string;
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
  const senders = input.senders ?? EMPTY_SENDERS;
  const inChatWindow =
    !!input.chatWindow &&
    String(input.chatWindow.groupId || "") === String(input.selectedGroupId || "");
  // A task's conversation takes the place of the filters while it is open.
  const taskId = inChatWindow ? "" : String(input.taskId || "").trim();
  const task = useTaskConversation(input.selectedGroupId, taskId);
  const pending = usePendingPages(
    input.selectedGroupId,
    input.chatFilter === "pending_user" && !inChatWindow && !taskId,
  );
  const pinned = usePinnedMessages(
    input.selectedGroupId,
    input.chatFilter === "pinned" && !inChatWindow && !taskId,
  );
  // Waiting for you and Pinned are server projections; they filter by sender themselves.
  const projection = pending.active || pinned.active || task.active;
  const senderHistory = useChatSenderHistory(
    input.selectedGroupId,
    projection ? EMPTY_SENDERS : senders,
  );
  const conversationEvents = senders.length === 0 ? input.events : senderHistory.events;
  const liveJump =
    !inChatWindow &&
    input.liveJump?.groupId === input.selectedGroupId &&
    input.chatFilter === "all" &&
    !senders.length &&
    !taskId
      ? input.liveJump
      : null;
  const viewKey =
    inChatWindow && input.chatWindow
      ? `${input.selectedGroupId}:window:${input.chatWindow.centerEventId}`
      : taskId
        ? `${input.selectedGroupId}:task:${taskId}`
        : `${input.selectedGroupId}:live:${input.chatFilter}:${JSON.stringify(senders)}${liveJump ? `:jump:${liveJump.requestId}` : ""}`;
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

  const unfilteredLiveEvents = useMemo(
    () => buildUnfilteredLiveChatMessages(input.events, input.outboxEntries, orderState),
    [input.events, input.outboxEntries, orderState],
  );
  const unfilteredLiveChatMessages = useMemo(
    () => unfilteredLiveEvents.map(toVisibleConversationEvent),
    [unfilteredLiveEvents],
  );

  const liveChatMessages = useMemo(() => {
    const canonical = conversationEvents
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
    const merged = mergeVisibleChatMessages(
      canonical,
      [],
      pending,
      senders.length ? { map: new Map(), next: 0 } : orderState,
    );
    const ordered = senders.length ? merged : sortChatMessages(merged, new Map());
    return ordered.filter((event) => matchesChatFilters(event, input.chatFilter, senders));
  }, [conversationEvents, input.chatFilter, input.outboxEntries, orderState, senders]);

  const chatMessages = useMemo(() => {
    if (!inChatWindow || !input.chatWindow) return liveChatMessages;
    return input.chatWindow.events
      .filter(isFormalChatMessageEvent)
      .filter(shouldShowInConversation)
      .map(toVisibleConversationEvent);
  }, [inChatWindow, input.chatWindow, liveChatMessages]);
  // Pending requests and pins are server projections over the complete ledger, not the
  // locally loaded tail. Keep closed or unpinned rows until the user leaves the view.
  // The task's messages come from the server search over the whole history and from the
  // loaded conversation, so one sent or received while the view is open joins it at once.
  const taskConversation = useMemo(
    () => (task.active ? collectTaskConversation(task.events, unfilteredLiveEvents, taskId) : null),
    [task.active, task.events, taskId, unfilteredLiveEvents],
  );
  const displayedMessages = useMemo(() => {
    if (taskConversation) return taskConversation.events.map(toVisibleConversationEvent);
    const projected = pending.active ? pending.events : pinned.active ? pinned.events : null;
    return projected
      ? projected.filter((event) => !senders.length || senders.includes(event.by ?? ""))
      : chatMessages;
  }, [
    taskConversation,
    pending.active,
    pending.events,
    pinned.active,
    pinned.events,
    senders,
    chatMessages,
  ]);

  const hasAnyChatMessages = useMemo(
    () =>
      input.events.some(
        (event) => isFormalChatMessageEvent(event) && shouldShowInConversation(event),
      ) ||
      input.outboxEntries.length > 0 ||
      senderHistory.events.length > 0,
    [input.events, input.outboxEntries, senderHistory.events.length],
  );
  const initialScroll = useMemo(
    () =>
      resolveInitialChatScroll({
        snapshot:
          !inChatWindow && shouldRestoreScrollSnapshot(input.scrollSnapshot, viewKey)
            ? input.scrollSnapshot!
            : null,
        viewKey,
        conversation: unfilteredLiveChatMessages,
        view: displayedMessages,
        exactOnly: projection,
      }),
    [
      displayedMessages,
      inChatWindow,
      input.scrollSnapshot,
      projection,
      unfilteredLiveChatMessages,
      viewKey,
    ],
  );
  const historyState = resolveChatListHistoryState({
    selectedGroupId: input.selectedGroupId,
    chatFilter: input.chatFilter,
    senderFiltered: senders.length > 0,
    filteredMessageCount: displayedMessages.length,
    hasAnyChatMessages,
    inChatWindow,
    hasLoadedTail: senders.length === 0 ? input.hasLoadedTail : senderHistory.loaded,
    hasMoreHistory: senders.length === 0 ? input.hasMoreHistory : senderHistory.hasMore,
    isLoadingHistory: senders.length === 0 ? input.isLoadingHistory : senderHistory.loading,
    isChatWindowLoading: input.isChatWindowLoading,
  });
  const effectiveIsLoadingHistory = task.active
    ? task.loading
    : pending.active
      ? pending.loading
      : pinned.active
        ? pinned.loading
        : historyState.isLoadingHistory;
  // All pins arrive in one read.
  const effectiveHasMoreHistory = task.active
    ? false
    : pending.active
      ? pending.hasMore
      : pinned.active
        ? false
        : historyState.hasMoreHistory;
  const hydratedDoc =
    !!input.groupDoc &&
    input.groupDoc.group_id === input.selectedGroupId &&
    (Object.prototype.hasOwnProperty.call(input.groupDoc, "scopes") ||
      Object.prototype.hasOwnProperty.call(input.groupDoc, "active_scope_key"));
  const settledActors =
    !!input.selectedGroupId &&
    (input.actors.length > 0 || input.groupContext !== null || input.groupSettings !== null);
  const emptyState: ChatEmptyState =
    displayedMessages.length > 0
      ? "ready"
      : task.active
        ? !task.loaded && !task.error
          ? "hydrating"
          : "filtered_empty"
        : pending.active
          ? !pending.loaded && !pending.error
            ? "hydrating"
            : "filtered_empty"
          : pinned.active
            ? !pinned.loaded && !pinned.error
              ? "hydrating"
              : "filtered_empty"
            : !input.selectedGroupId
              ? "business_empty"
              : historyState.isFilteredEmpty
                ? "filtered_empty"
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
    chatMessages: displayedMessages,
    hasAnyChatMessages,
    chatInitialScrollAnchorId: liveJump?.eventId || initialScroll.anchorId,
    // A jump opens at the reading top, never at another view's saved place.
    chatInitialScrollAnchorOffsetPx: liveJump ? undefined : initialScroll.anchorOffsetPx,
    chatInitialScrollOffsetPx: initialScroll.scrollOffsetPx,
    chatInitialScrollHighlight: !!liveJump || initialScroll.highlight === true,
    liveJumpActive: !!liveJump,
    chatWindowProps:
      inChatWindow && input.chatWindow
        ? {
            centerEventId: input.chatWindow.centerEventId,
            hasMoreBefore: input.chatWindow.hasMoreBefore,
            hasMoreAfter: input.chatWindow.hasMoreAfter,
            isLoadingAfter: input.chatWindow.isLoadingAfter === true,
            afterError: input.chatWindow.afterError === true,
          }
        : null,
    chatInitialScrollTargetId: centerEventId,
    chatHighlightEventId: centerEventId,
    effectiveIsLoadingHistory,
    effectiveHasMoreHistory,
    chatEmptyState: emptyState,
    senderHistoryError: senders.length === 0 ? "" : senderHistory.error,
    loadSenderHistory: senderHistory.loadMore,
    pendingActive: pending.active,
    // The projection itself read cleanly and holds no rows, whatever the sender filter shows.
    pendingEmpty: pending.active && pending.loaded && !pending.error && pending.events.length === 0,
    pinnedActive: pinned.active,
    pinnedError: pinned.active && pinned.error,
    pinnedUnavailable: pinned.active ? pinned.unavailable : EMPTY_IDS,
    retryPinned: pinned.refresh,
    pendingPageError: pending.active && pending.error,
    pendingDaemonUnsupported: pending.active && pending.error && pending.daemonUnsupported,
    loadPendingPage: pending.loadMore,
    retryPendingPage: pending.refresh,
    taskViewActive: task.active,
    taskViewLoading: task.active && task.loading,
    taskViewError: task.active && task.error,
    taskViewTruncated: task.active && task.truncated,
    taskViewContextIncomplete: !!taskConversation?.contextIncomplete,
    retryTaskView: task.refresh,
  };
}
