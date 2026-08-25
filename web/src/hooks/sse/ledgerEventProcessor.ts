import { useGroupStore, useModalStore, useUIStore } from "../../stores";
import type { Actor, ChatMessageData, LedgerEvent } from "../../types";
import {
  extractMailReadData,
  extractCancelledSourceEventId,
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
  actors: Actor[];
  activeTab: string;
  chatAtBottom: boolean;
  onContextSync: () => void;
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
  markPresentationSlotAttention: ModalState["markPresentationSlotAttention"];
  clearPresentationSlotAttention: ModalState["clearPresentationSlotAttention"];
};

export function processLedgerEvent(
  groupId: string,
  event: LedgerEvent,
  deps: LedgerEventProcessorDeps,
): void {
  if (isContextSyncEvent(event)) {
    deps.onContextSync();
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
        { actorId: data.actorId, deliveryState: data.state },
        groupId,
      );
    }
    return;
  }
  if (isReplyRequestCancelledEvent(event)) {
    const sourceEventId = extractCancelledSourceEventId(event);
    if (sourceEventId) {
      deps.updateObligationStatus(sourceEventId, { cancelled: true }, groupId);
    }
    return;
  }
  const reconciliation = reconcileCanonicalOutboxEvent(event, groupId);
  const nextEvent = reconciliation.event;
  initializeReadStatus(nextEvent, deps.actors);
  initializeObligationStatus(nextEvent, deps.actors);
  deps.appendEvent(nextEvent, groupId);

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
      deps.updateObligationStatus(replyTo, { actorId: replyBy, replied: true }, groupId);
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
    if (data?.message_mode === "mail") {
      const recipients = getRecipientActorIdsForEvent(nextEvent, deps.actors);
      if (recipients.length > 0) deps.incrementActorUnread(recipients);
    } else if (data?.message_mode === "send" || data?.message_mode === "request_reply") {
      const recipients = getRecipientActorIdsForEvent(nextEvent, deps.actors);
      if (recipients.length > 0) deps.incrementWebModelQueued(recipients);
    }
  }

  if (shouldIncrementUnread(nextEvent, deps.activeTab === "chat", deps.chatAtBottom)) {
    deps.incrementChatUnread(groupId);
  }
  const refreshMode = getActorRefreshMode(nextEvent);
  if (refreshMode === "unread") {
    void deps.refreshActors(groupId, { includeUnread: true });
  } else if (refreshMode === "readonly") {
    void deps.refreshActors(groupId, { includeUnread: false });
  }
}
