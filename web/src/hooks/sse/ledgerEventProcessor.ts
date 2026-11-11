import i18n from "../../i18n";
import { useGroupStore, useModalStore, useUIStore } from "../../stores";
import { groupMessagesVisible } from "../../stores/useUIStore";
import { signalBacklogChanged } from "../../stores/backlogSignals";
import type { Actor, ChatMessageData, LedgerEvent } from "../../types";
import {
  extractMailReadData,
  extractCancelledSourceEventId,
  extractFailedExit,
  extractRuntimeDeliveryData,
  getActorRefreshMode,
  getRecipientActorIdsForEvent,
  hasRenderableChatMessageContent,
  initializeObligationStatus,
  initializeReadStatus,
  isActorActivityEvent,
  isChatMessageEvent,
  isMailReadEvent,
  isReplyRequestCancelledEvent,
  isRuntimeDeliveryEvent,
  isContextSyncEvent,
  isPresentationClearEvent,
  isPresentationPublishEvent,
  shouldIncrementUnread,
} from "../../utils/ledgerEventHandlers";
import { getPresentationMessageRefs, getPresentationRefStatus } from "../../utils/presentationRefs";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "../../utils/chatOutboxReconciliation";
import {
  computeGroupRuntimeFromActorActivityUpdates,
  getRuntimeStatusFallbackForGroup,
} from "./runtimeStatus";

type GroupState = ReturnType<typeof useGroupStore.getState>;
type UiState = ReturnType<typeof useUIStore.getState>;
type ModalState = ReturnType<typeof useModalStore.getState>;

export type LedgerEventProcessorDeps = {
  alreadyProcessed?: boolean;
  actors: Actor[];
  activeTab: string;
  chatAtBottom: boolean;
  onContextSync: () => void;
  onGroupScopeChanged: () => void;
  appendEvent: GroupState["appendEvent"];
  updateReadStatus: GroupState["updateReadStatus"];
  updateObligationStatus: GroupState["updateObligationStatus"];
  incrementActorUnread: GroupState["incrementActorUnread"];
  incrementWebModelQueued: GroupState["incrementWebModelQueued"];
  updateActorActivity: GroupState["updateActorActivity"];
  updateGroupRuntimeState: GroupState["updateGroupRuntimeState"];
  promoteStreamingEventsByPrefix: GroupState["promoteStreamingEventsByPrefix"];
  removeStreamingEvent: GroupState["removeStreamingEvent"];
  clearEmptyStreamingEventsForActor: GroupState["clearEmptyStreamingEventsForActor"];
  refreshActors: GroupState["refreshActors"];
  refreshPresentation: GroupState["refreshPresentation"];
  incrementChatUnread: UiState["incrementChatUnread"];
  showError: UiState["showError"];
  markPresentationSlotAttention: ModalState["markPresentationSlotAttention"];
  clearPresentationSlotAttention: ModalState["clearPresentationSlotAttention"];
};

export function processLedgerEvent(
  groupId: string,
  event: LedgerEvent,
  deps: LedgerEventProcessorDeps,
): void {
  const eventId = String(event.id || "").trim();
  const bucket = useGroupStore.getState().chatByGroup[groupId];
  const alreadyApplied = !!eventId && !!deps.alreadyProcessed;
  // A sparse-view fetch can beat the live event without the reader reaching
  // it. It does not suppress the chat count; its actor badges need a refresh
  // because their last server snapshot may already include this event.
  const seenElsewhere =
    !!eventId &&
    [bucket?.events, bucket?.senderHistory?.events, bucket?.chatWindow?.events].some((events) =>
      events?.some((item) => item.id === eventId),
    );
  if (isContextSyncEvent(event)) {
    // A backlog write is a file change, not a Context write: it carries no version, and the
    // task board has nothing to read again unless the same event also changed tasks.
    const data = event.data as { backlog?: unknown; changes?: unknown } | undefined;
    if (data?.backlog) signalBacklogChanged(groupId);
    const changes = Array.isArray(data?.changes) ? data.changes : [];
    const backlogOnly =
      !!data?.backlog &&
      changes.length > 0 &&
      changes.every((change) =>
        String((change as { op?: unknown } | null)?.op || "").startsWith("backlog."),
      );
    if (!backlogOnly) deps.onContextSync();
    return;
  }
  if (isActorActivityEvent(event)) {
    const actors = event.data?.actors;
    if (Array.isArray(actors) && actors.length > 0) {
      const store = useGroupStore.getState();
      deps.updateActorActivity(actors, groupId);
      deps.updateGroupRuntimeState(
        groupId,
        computeGroupRuntimeFromActorActivityUpdates(
          deps.actors,
          actors,
          getRuntimeStatusFallbackForGroup(store, groupId),
        ),
      );
    }
    return;
  }
  if (isPresentationPublishEvent(event)) {
    void deps.refreshPresentation(groupId);
    const slotId = String(event.data?.slot_id || "").trim();
    if (slotId) deps.markPresentationSlotAttention(groupId, slotId);
    return;
  }
  if (isPresentationClearEvent(event)) {
    void deps.refreshPresentation(groupId);
    for (const slot of Array.isArray(event.data?.cleared_slots) ? event.data.cleared_slots : []) {
      const slotId = String(slot || "").trim();
      if (slotId) deps.clearPresentationSlotAttention(groupId, slotId);
    }
    return;
  }
  if (isMailReadEvent(event)) {
    const data = extractMailReadData(event);
    if (data) deps.updateReadStatus(data.eventId, data.actorId, groupId);
    if (getActorRefreshMode(event) === "unread") {
      void deps.refreshActors(groupId, { includeUnread: true });
    }
    return;
  }
  if (isRuntimeDeliveryEvent(event)) {
    const data = extractRuntimeDeliveryData(event);
    if (data) {
      deps.updateObligationStatus(
        data.eventId,
        { actorId: data.actorId, deliveryState: data.state, deliveryReason: data.reason },
        groupId,
      );
    }
    return;
  }
  if (isReplyRequestCancelledEvent(event)) {
    if (event.data?.connect_cancel) deps.appendEvent(event, groupId);
    const sourceEventId = extractCancelledSourceEventId(event);
    if (sourceEventId) {
      deps.updateObligationStatus(sourceEventId, { cancelled: true }, groupId);
    }
    return;
  }
  if (["group.set_active_scope", "group.attach", "group.detach_scope"].includes(event.kind || "")) {
    deps.onGroupScopeChanged();
  }
  const reconciliation = reconcileCanonicalOutboxEvent(event, groupId);
  const nextEvent = reconciliation.event;
  initializeReadStatus(nextEvent, deps.actors);
  initializeObligationStatus(nextEvent, deps.actors);
  // A read can beat the live event. appendEvent deduplicates the live list;
  // processed events may have been trimmed and must not be appended again.
  if (!deps.alreadyProcessed) deps.appendEvent(nextEvent, groupId);

  // Nothing else says so: Start Group would just look ignored.
  const exit = deps.alreadyProcessed ? null : extractFailedExit(nextEvent);
  if (exit) {
    const actor = deps.actors.find((item) => item.id === exit.actorId);
    deps.showError(
      i18n.t("actors:memberExited", {
        name: actor?.title || exit.actorId,
        code: exit.code,
        defaultValue: "{{name}} stopped (exit code {{code}}). Check its terminal.",
      }),
    );
  }

  if (isChatMessageEvent(nextEvent)) {
    const data = nextEvent.data as ChatMessageData;
    const streamId = String(data?.stream_id || "").trim();
    if (streamId) deps.removeStreamingEvent(streamId, groupId);

    const clientId = String(data?.client_id || "").trim();
    const canonicalEventId = String(nextEvent.id || "").trim();
    if (nextEvent.by === "user" && clientId && hasRenderableChatMessageContent(nextEvent)) {
      if (canonicalEventId) {
        deps.promoteStreamingEventsByPrefix(`local:${clientId}:`, canonicalEventId, groupId);
      }
      completeCanonicalOutboxReconciliation(groupId, reconciliation);
    }

    const replyTo = String(data?.reply_to || "").trim();
    const replyBy = String(nextEvent.by || "").trim();
    if (replyTo && replyBy) {
      deps.updateObligationStatus(replyTo, { reply: nextEvent }, groupId);
    }
    if (hasRenderableChatMessageContent(nextEvent) && replyBy && replyBy !== "user") {
      deps.clearEmptyStreamingEventsForActor(replyBy, groupId);
    }
    if (replyBy !== "user") {
      const needsAttention = data?.message_mode === "request_reply";
      for (const ref of getPresentationMessageRefs(data?.refs)) {
        if (needsAttention || getPresentationRefStatus(ref, data, event) === "needs_user") {
          const slotId = String(ref.slot_id || "").trim();
          if (slotId) deps.markPresentationSlotAttention(groupId, slotId);
        }
      }
    }
    if (!alreadyApplied && data?.message_mode === "mail") {
      const recipients = getRecipientActorIdsForEvent(nextEvent, deps.actors);
      if (recipients.length > 0) {
        if (seenElsewhere) void deps.refreshActors(groupId, { includeUnread: true });
        else deps.incrementActorUnread(recipients);
      }
    } else if (
      !alreadyApplied &&
      (data?.message_mode === "send" || data?.message_mode === "request_reply")
    ) {
      const recipients = getRecipientActorIdsForEvent(nextEvent, deps.actors);
      if (recipients.length > 0) {
        if (seenElsewhere) void deps.refreshActors(groupId, { includeUnread: true });
        else deps.incrementWebModelQueued(recipients);
      }
    }
  }

  if (
    !alreadyApplied &&
    shouldIncrementUnread(
      nextEvent,
      deps.activeTab === "chat" && groupMessagesVisible(groupId, useUIStore.getState()),
      deps.chatAtBottom,
    )
  ) {
    deps.incrementChatUnread(groupId);
  }
  const refreshMode = getActorRefreshMode(nextEvent);
  if (refreshMode === "unread") {
    void deps.refreshActors(groupId, { includeUnread: true });
  } else if (refreshMode === "readonly") {
    void deps.refreshActors(groupId, { includeUnread: false });
  }
}
