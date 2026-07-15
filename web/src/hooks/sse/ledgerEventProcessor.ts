import { useGroupStore, useModalStore, useUIStore } from "../../stores";
import type { Actor, ChatMessageData, LedgerEvent } from "../../types";
import {
  extractChatAckData,
  extractChatReadData,
  getActorRefreshMode,
  getRecipientActorIdsForEvent,
  hasRenderableChatMessageContent,
  initializeAckStatus,
  initializeObligationStatus,
  initializeReadStatus,
  isActorActivityEvent,
  isChatAckEvent,
  isChatMessageEvent,
  isChatReadEvent,
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
  updateAckStatus: GroupState["updateAckStatus"];
  updateReplyStatus: GroupState["updateReplyStatus"];
  incrementActorUnread: GroupState["incrementActorUnread"];
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

function getNotifyTargetActorId(event: LedgerEvent): string {
  if (event.kind !== "system.notify" || !event.data || typeof event.data !== "object") return "";
  const actorId = String(
    (event.data as { target_actor_id?: unknown }).target_actor_id || "",
  ).trim();
  return actorId && actorId !== "user" ? actorId : "";
}

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
      deps.updateActorActivity(actors);
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
  if (isChatReadEvent(event)) {
    const data = extractChatReadData(event);
    if (data) deps.updateReadStatus(data.eventId, data.actorId, groupId);
    if (getActorRefreshMode(event) === "unread") {
      void deps.refreshActors(groupId, { includeUnread: true });
    }
    return;
  }
  if (isChatAckEvent(event)) {
    const data = extractChatAckData(event);
    if (data) deps.updateAckStatus(data.eventId, data.actorId, groupId);
    return;
  }

  const reconciliation = reconcileCanonicalOutboxEvent(event, groupId);
  const nextEvent = reconciliation.event;
  initializeReadStatus(nextEvent, deps.actors);
  initializeAckStatus(nextEvent, deps.actors);
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
    if (replyTo && replyBy) deps.updateReplyStatus(replyTo, replyBy, groupId);
    if (hasRenderableChatMessageContent(nextEvent) && replyBy && replyBy !== "user") {
      deps.clearEmptyStreamingEventsForActor(replyBy, groupId);
    }
    if (replyBy !== "user") {
      const needsAttention =
        String(data?.priority || "normal").trim() === "attention" || !!data?.reply_required;
      for (const ref of getPresentationMessageRefs(data?.refs)) {
        if (needsAttention || getPresentationRefStatus(ref, data, event) === "needs_user") {
          const slotId = String(ref.slot_id || "").trim();
          if (slotId) deps.markPresentationSlotAttention(groupId, slotId);
        }
      }
    }
    const recipients = getRecipientActorIdsForEvent(nextEvent, deps.actors);
    if (recipients.length > 0) deps.incrementActorUnread(recipients);
  }

  if (shouldIncrementUnread(nextEvent, deps.activeTab === "chat", deps.chatAtBottom)) {
    deps.incrementChatUnread(groupId);
  }
  const notifyActorId = getNotifyTargetActorId(nextEvent);
  if (notifyActorId) deps.incrementActorUnread([notifyActorId]);
  const refreshMode = getActorRefreshMode(nextEvent);
  if (refreshMode === "unread") {
    void deps.refreshActors(groupId, { includeUnread: true });
  } else if (refreshMode === "readonly") {
    void deps.refreshActors(groupId, { includeUnread: false });
  }
}
