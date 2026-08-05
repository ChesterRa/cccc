// SSE connection management for the ledger stream.
import { useEffect, useRef } from "react";
import { useGroupStore, useUIStore, useModalStore } from "../stores";
import { mergeStreamingActivity } from "../stores/chatStreamingSessions";
import { beginContextRequest, isLatestContextRequest } from "../stores/groupStoreCore";
import * as api from "../services/api";
import type { FetchContextOptions } from "../services/api";
import type { HeadlessStreamEvent, GroupContext, LedgerEvent, StreamingActivity } from "../types";
import { runReconnectCatchup, scheduleContextSummaryCatchup } from "./sseCatchup";
import {
  getRecipientActorIdsForEvent,
  getAckRecipientIdsForEvent,
} from "../utils/ledgerEventHandlers";
import { replayHeadlessSnapshotEvents } from "../utils/headlessSnapshotReplay";
import { isHeadlessActorRunner } from "../utils/headlessRuntimeSupport";
import { createSseConnectionRegistry } from "./sseConnectionRegistry";
import {
  computeGroupRuntimeFromActorActivityUpdate,
  getRuntimeStatusFallbackForGroup,
  type ActorActivityUpdate,
} from "./sse/runtimeStatus";
import {
  formatHeadlessErrorMessage,
  headlessActorKey,
  translateActorLabel,
} from "./sse/headlessEventUtils";
import { processLedgerEvent } from "./sse/ledgerEventProcessor";
import { reconcileLedgerTail } from "./sse/reconcileLedgerTail";
import { getGroupStreamsHiddenDisconnectDelayMs, shouldStartGroupStreams } from "./sse/visibility";
import type { UseSSEOptions } from "./sse/types";

export {
  computeGroupRuntimeFromActorActivityUpdate,
  computeGroupRuntimeFromActorActivityUpdates,
  getRuntimeStatusFallbackForGroup,
} from "./sse/runtimeStatus";
export {
  GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS,
  getGroupStreamsHiddenDisconnectDelayMs,
  shouldStartGroupStreams,
} from "./sse/visibility";

export { getRecipientActorIdsForEvent, getAckRecipientIdsForEvent };

export function useSSE({ activeTabRef, chatAtBottomRef, actorsRef }: UseSSEOptions) {
  const selectedGroupId = useGroupStore((s) => s.selectedGroupId);
  const appendEvent = useGroupStore((s) => s.appendEvent);
  const appendHeadlessEvent = useGroupStore((s) => s.appendHeadlessEvent);
  const updateReadStatus = useGroupStore((s) => s.updateReadStatus);
  const updateAckStatus = useGroupStore((s) => s.updateAckStatus);
  const updateReplyStatus = useGroupStore((s) => s.updateReplyStatus);
  const incrementActorUnread = useGroupStore((s) => s.incrementActorUnread);
  const updateActorActivity = useGroupStore((s) => s.updateActorActivity);
  const upsertStreamingActivity = useGroupStore((s) => s.upsertStreamingActivity);
  const promoteStreamingEventToStream = useGroupStore((s) => s.promoteStreamingEventToStream);
  const promoteStreamingEventsByPrefix = useGroupStore((s) => s.promoteStreamingEventsByPrefix);
  const reconcileStreamingMessage = useGroupStore((s) => s.reconcileStreamingMessage);
  const completeStreamingEventsForActor = useGroupStore((s) => s.completeStreamingEventsForActor);
  const removeStreamingEvent = useGroupStore((s) => s.removeStreamingEvent);
  const clearStreamingEventsForActor = useGroupStore((s) => s.clearStreamingEventsForActor);
  const clearEmptyStreamingEventsForActor = useGroupStore(
    (s) => s.clearEmptyStreamingEventsForActor,
  );
  const clearTransientStreamingEventsForActor = useGroupStore(
    (s) => s.clearTransientStreamingEventsForActor,
  );
  const setGroupContext = useGroupStore((s) => s.setGroupContext);
  const updateGroupRuntimeState = useGroupStore((s) => s.updateGroupRuntimeState);
  const refreshActors = useGroupStore((s) => s.refreshActors);
  const refreshPresentation = useGroupStore((s) => s.refreshPresentation);

  const incrementChatUnread = useUIStore((s) => s.incrementChatUnread);
  const setSSEStatus = useUIStore((s) => s.setSSEStatus);
  const markPresentationSlotAttention = useModalStore((s) => s.markPresentationSlotAttention);
  const clearPresentationSlotAttention = useModalStore((s) => s.clearPresentationSlotAttention);

  const eventSourceRef = useRef<EventSource | null>(null);
  const headlessEventSourceRef = useRef<EventSource | null>(null);
  const sseRegistryRef = useRef(createSseConnectionRegistry<EventSource>());
  const contextRefreshTimerRef = useRef<number | null>(null);
  const selectedGroupIdRef = useRef<string>("");
  const headlessReconnectDelayRef = useRef<number>(1000);
  const headlessReconnectTimerRef = useRef<number | null>(null);
  const hiddenDisconnectTimerRef = useRef<number | null>(null);
  const hasConnectedOnceRef = useRef<boolean>(false);
  const needsVisibilityCatchupRef = useRef<boolean>(false);
  const headlessThreadIdByActorRef = useRef(new Map<string, string>());
  const pendingHeadlessMessageFlushRef = useRef<number | null>(null);
  const pendingHeadlessActivityFlushRef = useRef<number | null>(null);
  const pendingHeadlessMessagesRef = useRef(
    new Map<
      string,
      {
        groupId: string;
        actorId: string;
        streamId: string;
        pendingEventId: string;
        ts: string;
        explicitText: string | null;
        deltaText: string;
        completed: boolean;
        shouldClearPlaceholder: boolean;
        transientStream: boolean;
        phase: string;
      }
    >(),
  );
  const pendingHeadlessActivitiesRef = useRef(
    new Map<
      string,
      {
        actorId: string;
        groupId: string;
        match: { pendingEventId?: string; streamId?: string };
        activities: Map<string, StreamingActivity>;
      }
    >(),
  );

  useEffect(() => {
    selectedGroupIdRef.current = selectedGroupId;
  }, [selectedGroupId]);

  async function fetchContext(groupId: string, opts?: FetchContextOptions) {
    if (opts?.fresh && contextRefreshTimerRef.current) {
      window.clearTimeout(contextRefreshTimerRef.current);
      contextRefreshTimerRef.current = null;
    }
    const contextEpoch = beginContextRequest(groupId);
    const resp = await api.fetchContext(groupId, {
      fresh: opts?.fresh,
      detail: opts?.detail ?? "summary",
    });
    if (
      resp.ok &&
      resp.result &&
      typeof resp.result === "object" &&
      selectedGroupIdRef.current === groupId &&
      isLatestContextRequest(groupId, contextEpoch)
    ) {
      setGroupContext(resp.result as GroupContext);
    }
  }

  async function resyncAfterReconnect(groupId: string) {
    await runReconnectCatchup(groupId, {
      invalidateContextRead: api.invalidateContextRead,
      reconcileLedgerTail: (gid) =>
        reconcileLedgerTail(gid, () => selectedGroupIdRef.current === gid),
      refreshActors,
      fetchContextSummary: fetchContext,
    });
  }

  function flushPendingHeadlessMessages(targetGroupId?: string, targetActorId?: string) {
    if (targetGroupId == null && targetActorId == null) {
      pendingHeadlessMessageFlushRef.current = null;
    }
    const pendingEntries = pendingHeadlessMessagesRef.current;
    if (pendingEntries.size <= 0) return;

    for (const [key, entry] of pendingEntries.entries()) {
      if (targetGroupId && entry.groupId !== targetGroupId) continue;
      if (targetActorId && entry.actorId !== targetActorId) continue;
      const streamingEvents =
        useGroupStore.getState().chatByGroup[entry.groupId]?.streamingEvents || [];
      const placeholder = entry.pendingEventId
        ? streamingEvents.find((item) => {
            if (String(item.by || "").trim() !== entry.actorId) return false;
            const itemData =
              item.data && typeof item.data === "object"
                ? (item.data as { pending_event_id?: unknown; pending_placeholder?: unknown })
                : undefined;
            return (
              Boolean(itemData?.pending_placeholder) &&
              String(itemData?.pending_event_id || "").trim() === entry.pendingEventId
            );
          })
        : undefined;
      const existing = streamingEvents.find((item) => {
        const itemStreamId =
          item.data && typeof item.data === "object"
            ? String((item.data as { stream_id?: unknown }).stream_id || "").trim()
            : "";
        return itemStreamId === entry.streamId;
      });
      const bucket = useGroupStore.getState().chatByGroup[entry.groupId];
      const previousStreamText = String(bucket?.streamingTextByStreamId?.[entry.streamId] || "");
      const previousEventText =
        existing?.data && typeof existing.data === "object"
          ? String((existing.data as { text?: unknown }).text || "")
          : "";
      const existingData =
        existing?.data && typeof existing.data === "object"
          ? (existing.data as {
              pending_event_id?: unknown;
              pending_placeholder?: unknown;
              text?: unknown;
              transient_stream?: unknown;
              stream_phase?: unknown;
            })
          : undefined;
      const previousPhase = String(existingData?.stream_phase || "")
        .trim()
        .toLowerCase();
      const nextPhase = String(entry.phase || "")
        .trim()
        .toLowerCase();
      const hasIncomingPhaseText =
        entry.explicitText != null ? entry.explicitText.length > 0 : entry.deltaText.length > 0;
      const shouldResetTextForPhaseTransition =
        !!nextPhase &&
        previousPhase !== nextPhase &&
        previousPhase.length > 0 &&
        hasIncomingPhaseText;
      const previousText = shouldResetTextForPhaseTransition
        ? ""
        : previousStreamText || previousEventText;
      const previousActivities = (() => {
        const source = existing ?? placeholder;
        if (!source?.data || typeof source.data !== "object") return [];
        const activities = (source.data as { activities?: unknown }).activities;
        return Array.isArray(activities) ? activities : [];
      })();
      const fullText = entry.explicitText ?? `${previousText}${entry.deltaText}`;
      const nextPlaceholderState = !fullText.trim() && previousActivities.length <= 0;
      const nextEventText = fullText
        ? fullText
        : shouldResetTextForPhaseTransition
          ? ""
          : previousEventText;
      const existingPendingEventId = String(existingData?.pending_event_id || "").trim();
      const needsEventUpsert =
        !existing ||
        !!existing._streaming !== !entry.completed ||
        existingPendingEventId !== entry.pendingEventId ||
        String(existingData?.text || "") !== nextEventText ||
        Boolean(existingData?.transient_stream) !== entry.transientStream ||
        String(existingData?.stream_phase || "") !== entry.phase ||
        Boolean(existingData?.pending_placeholder) !== nextPlaceholderState ||
        previousStreamText !== fullText;
      if (needsEventUpsert) {
        reconcileStreamingMessage({
          actorId: entry.actorId,
          pendingEventId: entry.pendingEventId,
          streamId: entry.streamId,
          ts: entry.ts,
          fullText,
          eventText: nextEventText,
          activities: previousActivities,
          completed: entry.completed,
          transientStream: entry.transientStream,
          phase: entry.phase || undefined,
          groupId: entry.groupId,
        });
      }
      pendingEntries.delete(key);
    }
  }

  function schedulePendingHeadlessMessageFlush(groupId: string) {
    if (pendingHeadlessMessageFlushRef.current != null) return;
    pendingHeadlessMessageFlushRef.current = window.requestAnimationFrame(() => {
      pendingHeadlessMessageFlushRef.current = null;
      flushPendingHeadlessMessages(groupId);
    });
  }

  function flushPendingHeadlessActivities(targetGroupId?: string, targetActorId?: string) {
    if (targetGroupId == null && targetActorId == null) {
      pendingHeadlessActivityFlushRef.current = null;
    }
    const pendingEntries = pendingHeadlessActivitiesRef.current;
    if (pendingEntries.size <= 0) return;

    for (const [key, entry] of pendingEntries.entries()) {
      if (targetGroupId && entry.groupId !== targetGroupId) continue;
      if (targetActorId && entry.actorId !== targetActorId) continue;
      for (const activity of entry.activities.values()) {
        upsertStreamingActivity(entry.actorId, entry.match, activity, entry.groupId);
      }
      pendingEntries.delete(key);
    }
  }

  function schedulePendingHeadlessActivityFlush() {
    if (pendingHeadlessActivityFlushRef.current != null) return;
    pendingHeadlessActivityFlushRef.current = window.requestAnimationFrame(() => {
      pendingHeadlessActivityFlushRef.current = null;
      flushPendingHeadlessActivities();
    });
  }

  function clearPendingHeadlessBuffers(groupId: string, actorId: string) {
    const targetGroupId = String(groupId || "").trim();
    const targetActorId = String(actorId || "").trim();
    if (!targetGroupId || !targetActorId) return;

    for (const [key, entry] of pendingHeadlessMessagesRef.current.entries()) {
      if (key.startsWith(`${targetGroupId}:`) && entry.actorId === targetActorId) {
        pendingHeadlessMessagesRef.current.delete(key);
      }
    }

    for (const [key, entry] of pendingHeadlessActivitiesRef.current.entries()) {
      if (entry.groupId === targetGroupId && entry.actorId === targetActorId) {
        pendingHeadlessActivitiesRef.current.delete(key);
      }
    }

    if (
      pendingHeadlessMessagesRef.current.size === 0 &&
      pendingHeadlessMessageFlushRef.current != null
    ) {
      window.cancelAnimationFrame(pendingHeadlessMessageFlushRef.current);
      pendingHeadlessMessageFlushRef.current = null;
    }
    if (
      pendingHeadlessActivitiesRef.current.size === 0 &&
      pendingHeadlessActivityFlushRef.current != null
    ) {
      window.cancelAnimationFrame(pendingHeadlessActivityFlushRef.current);
      pendingHeadlessActivityFlushRef.current = null;
    }
  }

  function clearHeadlessLiveOutput(groupId: string, actorId: string) {
    const targetGroupId = String(groupId || "").trim();
    const targetActorId = String(actorId || "").trim();
    if (!targetGroupId || !targetActorId) return;
    clearPendingHeadlessBuffers(targetGroupId, targetActorId);
    clearStreamingEventsForActor(targetActorId, targetGroupId);
  }

  function reconcileHydratedHeadlessLiveOutput(groupId: string, events: HeadlessStreamEvent[]) {
    const targetGroupId = String(groupId || "").trim();
    if (!targetGroupId) return;
    const snapshotActorIds = new Set<string>();
    for (const event of Array.isArray(events) ? events : []) {
      const actorId = String(event?.actor_id || "").trim();
      if (actorId) snapshotActorIds.add(actorId);
    }
    const bucket = useGroupStore.getState().chatByGroup[targetGroupId];
    const liveActorIds = new Set<string>();
    for (const actor of actorsRef.current) {
      if (!isHeadlessActorRunner(actor)) continue;
      const actorId = String(actor.id || "").trim();
      if (!actorId) continue;
      const hasLiveStream =
        Array.isArray(bucket?.streamingEvents) &&
        bucket.streamingEvents.some(
          (event) => String(event.by || "").trim() === actorId && !!event._streaming,
        );
      if (hasLiveStream) {
        liveActorIds.add(actorId);
      }
    }

    for (const actorId of liveActorIds) {
      if (snapshotActorIds.has(actorId)) continue;
      clearHeadlessLiveOutput(targetGroupId, actorId);
      headlessThreadIdByActorRef.current.delete(headlessActorKey(targetGroupId, actorId));
    }
  }

  function handleHeadlessEvent(groupId: string, ev: HeadlessStreamEvent) {
    try {
      const actorId = String(ev.actor_id || "").trim();
      const eventType = String(ev.type || "").trim();
      const data = ev.data && typeof ev.data === "object" ? ev.data : {};
      const streamId = typeof data.stream_id === "string" ? data.stream_id.trim() : "";
      const pendingEventId = typeof data.event_id === "string" ? data.event_id.trim() : "";
      if (!actorId || !eventType) return;
      appendHeadlessEvent(ev, groupId);

      function updateHeadlessActorRuntime(update: ActorActivityUpdate) {
        const storeState = useGroupStore.getState();
        const actorsSnapshot = storeState.actors;
        updateActorActivity([update]);
        updateGroupRuntimeState(
          groupId,
          computeGroupRuntimeFromActorActivityUpdate(
            actorsSnapshot.length > 0 ? actorsSnapshot : actorsRef.current,
            update,
            getRuntimeStatusFallbackForGroup(storeState, groupId),
          ),
        );
      }

      function queueHeadlessActivity(activity: StreamingActivity) {
        const activityKey = `${groupId}:${actorId}:${streamId || pendingEventId || "pending"}`;
        const existingActivityBatch = pendingHeadlessActivitiesRef.current.get(activityKey);
        if (existingActivityBatch) {
          existingActivityBatch.match = { pendingEventId, streamId };
          const existingActivity = existingActivityBatch.activities.get(activity.id);
          existingActivityBatch.activities.set(
            activity.id,
            mergeStreamingActivity(existingActivity, activity) || activity,
          );
        } else {
          pendingHeadlessActivitiesRef.current.set(activityKey, {
            actorId,
            groupId,
            match: { pendingEventId, streamId },
            activities: new Map([[activity.id, activity]]),
          });
        }
        schedulePendingHeadlessActivityFlush();
      }

      if (
        eventType === "headless.thread.resume_failed" ||
        eventType === "headless.session.resume_failed"
      ) {
        const resumeError = String(data.error || data.message || data.detail || "").trim();
        updateHeadlessActorRuntime({
          id: actorId,
          running: false,
          idle_seconds: null,
          effective_working_state: "blocked",
          effective_working_reason: "headless_runtime_resume_failed",
          effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
          effective_active_task_id: null,
          runtime_session_status: "resume_failed",
          runtime_session_resume_eligible: false,
          runtime_session_last_resume_error: resumeError || null,
        });
        return;
      }

      if (eventType === "headless.thread.started" || eventType === "headless.thread.resumed") {
        const threadId = typeof data.thread_id === "string" ? data.thread_id.trim() : "";
        const actorKey = headlessActorKey(groupId, actorId);
        const previousThreadId = String(
          headlessThreadIdByActorRef.current.get(actorKey) || "",
        ).trim();
        if (eventType === "headless.thread.started" && threadId && threadId !== previousThreadId) {
          clearHeadlessLiveOutput(groupId, actorId);
        }
        if (threadId) {
          headlessThreadIdByActorRef.current.set(actorKey, threadId);
        }
        updateHeadlessActorRuntime({
          id: actorId,
          running: true,
          idle_seconds: null,
          effective_working_state: "idle",
          effective_working_reason:
            eventType === "headless.thread.resumed"
              ? "headless_thread_resumed"
              : "headless_thread_started",
          effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
          effective_active_task_id: null,
          runtime_session_status: "usable",
          runtime_session_resume_eligible: true,
          runtime_session_last_resume_error: null,
        });
        return;
      }

      if (eventType === "headless.session.stopped") {
        clearHeadlessLiveOutput(groupId, actorId);
        headlessThreadIdByActorRef.current.delete(headlessActorKey(groupId, actorId));
        updateHeadlessActorRuntime({
          id: actorId,
          running: false,
          idle_seconds: null,
          effective_working_state: "stopped",
          effective_working_reason: "headless_session_stopped",
          effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
          effective_active_task_id: null,
        });
        return;
      }

      if (eventType === "headless.turn.started" || eventType === "headless.turn.progress") {
        updateHeadlessActorRuntime({
          id: actorId,
          running: true,
          idle_seconds: null,
          effective_working_state: "working",
          effective_working_reason: "headless_turn_active",
          effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
          effective_active_task_id: typeof data.turn_id === "string" ? data.turn_id : null,
        });
        return;
      }

      if (eventType === "headless.turn.stalled") {
        updateHeadlessActorRuntime({
          id: actorId,
          running: true,
          idle_seconds: null,
          effective_working_state: "waiting",
          effective_working_reason: "headless_turn_stalled",
          effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
          effective_active_task_id: typeof data.turn_id === "string" ? data.turn_id : null,
        });
        return;
      }

      if (eventType === "headless.turn.completed" || eventType === "headless.turn.failed") {
        const failed = eventType === "headless.turn.failed";
        const turnId = typeof data.turn_id === "string" ? data.turn_id.trim() : "";
        const errorMessage = formatHeadlessErrorMessage(data.error);
        flushPendingHeadlessActivities(groupId, actorId);
        flushPendingHeadlessMessages(groupId, actorId);
        clearPendingHeadlessBuffers(groupId, actorId);
        completeStreamingEventsForActor(actorId, groupId);
        clearTransientStreamingEventsForActor(actorId, groupId);
        updateHeadlessActorRuntime({
          id: actorId,
          running: true,
          idle_seconds: null,
          effective_working_state: "idle",
          effective_working_reason: failed ? "headless_turn_failed" : "headless_turn_idle",
          effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
          effective_active_task_id: null,
        });
        if (failed) {
          clearStreamingEventsForActor(actorId, groupId);
          if (pendingEventId) {
            upsertStreamingActivity(
              actorId,
              { pendingEventId, streamId: streamId || turnId },
              {
                id: `error:${pendingEventId || turnId || actorId}`,
                kind: "error",
                status: "completed",
                summary: translateActorLabel("headlessTurnFailed", "Model request failed"),
                detail:
                  errorMessage ||
                  translateActorLabel(
                    "headlessTurnFailedFallback",
                    "The model runtime reported an error.",
                  ),
                ts: typeof ev.ts === "string" ? ev.ts : new Date().toISOString(),
                raw_item_type: "turn_error",
              },
              groupId,
            );
          }
        } else {
          clearEmptyStreamingEventsForActor(actorId, groupId);
        }
        return;
      }

      if (
        eventType === "headless.control.queued" ||
        eventType === "headless.control.started" ||
        eventType === "headless.control.requeued" ||
        eventType === "headless.control.stalled" ||
        eventType === "headless.control.completed" ||
        eventType === "headless.control.failed"
      ) {
        const controlKind =
          typeof data.control_kind === "string" ? data.control_kind.trim() : "control";
        const turnId = typeof data.turn_id === "string" ? data.turn_id.trim() : "";
        const controlEventId = typeof data.event_id === "string" ? data.event_id.trim() : "";
        const errorMessage = formatHeadlessErrorMessage(data.error);
        const controlStatusLabel = (() => {
          switch (eventType) {
            case "headless.control.queued":
              return translateActorLabel("headlessControlQueued", "Control task queued");
            case "headless.control.started":
              return translateActorLabel("headlessControlStarted", "Processing control task");
            case "headless.control.requeued":
              return translateActorLabel("headlessControlRequeued", "Retrying control task");
            case "headless.control.stalled":
              return translateActorLabel("headlessControlStalled", "Control task is waiting");
            case "headless.control.completed":
              return translateActorLabel("headlessControlCompleted", "Control task completed");
            case "headless.control.failed":
              return translateActorLabel("headlessControlFailed", "Control task failed");
            default:
              return translateActorLabel("headlessControlUpdated", "Control task updated");
          }
        })();
        const activity: StreamingActivity = {
          id: `control:${controlEventId || turnId || controlKind || actorId}`,
          kind: eventType === "headless.control.queued" ? "queued" : "thinking",
          status:
            eventType === "headless.control.completed" || eventType === "headless.control.failed"
              ? "completed"
              : eventType === "headless.control.started"
                ? "started"
                : "updated",
          summary: controlStatusLabel,
          detail:
            [controlKind ? `kind ${controlKind}` : "", errorMessage].filter(Boolean).join(" | ") ||
            undefined,
          ts: typeof ev.ts === "string" ? ev.ts : new Date().toISOString(),
        };
        queueHeadlessActivity(activity);

        if (eventType === "headless.control.completed" || eventType === "headless.control.failed") {
          flushPendingHeadlessActivities(groupId, actorId);
          flushPendingHeadlessMessages(groupId, actorId);
          clearPendingHeadlessBuffers(groupId, actorId);
          updateHeadlessActorRuntime({
            id: actorId,
            running: true,
            idle_seconds: null,
            effective_working_state: "idle",
            effective_working_reason:
              eventType === "headless.control.failed"
                ? "headless_control_failed"
                : "headless_control_idle",
            effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
            effective_active_task_id: null,
          });
          clearEmptyStreamingEventsForActor(actorId, groupId);
        } else if (eventType === "headless.control.stalled") {
          updateHeadlessActorRuntime({
            id: actorId,
            running: true,
            idle_seconds: null,
            effective_working_state: "waiting",
            effective_working_reason: "headless_control_stalled",
            effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
            effective_active_task_id: turnId || controlEventId || null,
          });
        } else {
          updateHeadlessActorRuntime({
            id: actorId,
            running: true,
            idle_seconds: null,
            effective_working_state: "working",
            effective_working_reason: "headless_control_active",
            effective_working_updated_at: typeof ev.ts === "string" ? ev.ts : null,
            effective_active_task_id: turnId || controlEventId || null,
          });
        }
        return;
      }

      if (
        eventType === "headless.activity.started" ||
        eventType === "headless.activity.updated" ||
        eventType === "headless.activity.completed"
      ) {
        const activityId = typeof data.activity_id === "string" ? data.activity_id.trim() : "";
        const summary = typeof data.summary === "string" ? data.summary.trim() : "";
        if (!activityId || !summary) return;
        const activityTs = typeof ev.ts === "string" ? ev.ts : new Date().toISOString();
        const activity: StreamingActivity = {
          id: activityId,
          kind: typeof data.kind === "string" ? data.kind.trim() : "thinking",
          status: eventType.replace("headless.activity.", ""),
          summary,
          detail: typeof data.detail === "string" ? data.detail.trim() : undefined,
          ts: activityTs,
          raw_item_type:
            typeof data.raw_item_type === "string" ? data.raw_item_type.trim() : undefined,
          tool_name: typeof data.tool_name === "string" ? data.tool_name.trim() : undefined,
          server_name: typeof data.server_name === "string" ? data.server_name.trim() : undefined,
          command: typeof data.command === "string" ? data.command.trim() : undefined,
          cwd: typeof data.cwd === "string" ? data.cwd.trim() : undefined,
          file_paths: Array.isArray(data.file_paths)
            ? data.file_paths.map((item) => String(item || "").trim()).filter((item) => item)
            : undefined,
          query: typeof data.query === "string" ? data.query.trim() : undefined,
        };
        queueHeadlessActivity(activity);
        return;
      }

      if (
        eventType === "headless.message.started" ||
        eventType === "headless.message.delta" ||
        eventType === "headless.message.completed"
      ) {
        if (!streamId) return;
        const delta = typeof data.delta === "string" ? data.delta : "";
        const explicitTextRaw = typeof data.text === "string" ? data.text : null;
        const explicitText =
          explicitTextRaw === "" && eventType === "headless.message.started"
            ? null
            : explicitTextRaw;
        const phase = typeof data.phase === "string" ? data.phase.trim().toLowerCase() : "";
        const transientStream = !!phase && phase !== "final_answer";
        const shouldBindToPendingPlaceholder = !!pendingEventId;
        if (pendingEventId && shouldBindToPendingPlaceholder) {
          promoteStreamingEventToStream(actorId, pendingEventId, streamId, groupId);
        }
        const messageKey = `${groupId}:${streamId}`;
        const existingMessageBatch = pendingHeadlessMessagesRef.current.get(messageKey);
        if (existingMessageBatch) {
          existingMessageBatch.pendingEventId =
            pendingEventId || existingMessageBatch.pendingEventId;
          existingMessageBatch.ts = typeof ev.ts === "string" ? ev.ts : existingMessageBatch.ts;
          existingMessageBatch.transientStream = transientStream;
          existingMessageBatch.phase = phase || existingMessageBatch.phase;
          if (explicitText != null) {
            existingMessageBatch.explicitText = explicitText;
            existingMessageBatch.deltaText = "";
          } else if (delta) {
            existingMessageBatch.deltaText += delta;
          }
          if (pendingEventId && shouldBindToPendingPlaceholder) {
            existingMessageBatch.shouldClearPlaceholder = true;
          }
          if (eventType === "headless.message.completed") {
            existingMessageBatch.completed = true;
          }
        } else {
          pendingHeadlessMessagesRef.current.set(messageKey, {
            groupId,
            actorId,
            streamId,
            pendingEventId,
            ts: typeof ev.ts === "string" ? ev.ts : new Date().toISOString(),
            explicitText,
            deltaText: explicitText == null ? delta : "",
            completed: eventType === "headless.message.completed",
            shouldClearPlaceholder: !!pendingEventId && shouldBindToPendingPlaceholder,
            transientStream,
            phase,
          });
        }
        schedulePendingHeadlessMessageFlush(groupId);
      }
    } catch {
      /* ignore parse errors */
    }
  }

  async function hydrateHeadlessSnapshot(groupId: string) {
    const resp = await api.fetchHeadlessSnapshot(groupId, { noCache: true });
    if (!resp.ok || selectedGroupIdRef.current !== groupId) return;
    const events = Array.isArray(resp.result.events) ? resp.result.events : [];
    reconcileHydratedHeadlessLiveOutput(groupId, events);
    replayHeadlessSnapshotEvents(events, (event) => {
      handleHeadlessEvent(groupId, event);
    });
    flushPendingHeadlessActivities(groupId);
    flushPendingHeadlessMessages(groupId);
  }

  function closeLedgerStream() {
    sseRegistryRef.current.close("ledger");
    eventSourceRef.current = null;
  }

  function closeHeadlessStream() {
    sseRegistryRef.current.close("headless");
    headlessEventSourceRef.current = null;
  }

  function connectHeadlessStream(groupId: string, options?: { replay?: boolean }) {
    if (headlessReconnectTimerRef.current) {
      window.clearTimeout(headlessReconnectTimerRef.current);
      headlessReconnectTimerRef.current = null;
    }
    closeHeadlessStream();

    const replay = options?.replay !== false;
    const params = new URLSearchParams();
    if (!replay) params.set("replay", "false");
    const headlessPath = `/api/v1/groups/${encodeURIComponent(groupId)}/headless/stream${params.toString() ? `?${params.toString()}` : ""}`;
    const headlessEs = new EventSource(api.withAuthToken(headlessPath));
    const headlessToken = sseRegistryRef.current.set("headless", groupId, headlessEs);
    headlessEs.onopen = () => {
      if (!sseRegistryRef.current.isCurrent(headlessToken)) return;
      headlessReconnectDelayRef.current = 1000;
    };
    headlessEs.onerror = () => {
      if (!sseRegistryRef.current.isCurrent(headlessToken)) return;
      closeHeadlessStream();
      if (headlessReconnectTimerRef.current) {
        window.clearTimeout(headlessReconnectTimerRef.current);
      }
      const delay = headlessReconnectDelayRef.current;
      headlessReconnectTimerRef.current = window.setTimeout(() => {
        headlessReconnectTimerRef.current = null;
        if (selectedGroupIdRef.current === groupId) {
          connectHeadlessStream(groupId, { replay: true });
        }
      }, delay);
      headlessReconnectDelayRef.current = Math.min(delay * 2, 30000);
    };
    headlessEs.addEventListener("headless", (e) => {
      if (!sseRegistryRef.current.isCurrent(headlessToken)) return;
      const msg = e as MessageEvent;
      try {
        handleHeadlessEvent(groupId, JSON.parse(String(msg.data || "{}")) as HeadlessStreamEvent);
      } catch {
        /* ignore parse errors */
      }
    });
    headlessEventSourceRef.current = headlessEs;
  }

  function clearHiddenGroupStreamDisconnectTimer() {
    if (hiddenDisconnectTimerRef.current == null) return;
    window.clearTimeout(hiddenDisconnectTimerRef.current);
    hiddenDisconnectTimerRef.current = null;
  }

  function scheduleHiddenGroupStreamDisconnect() {
    clearHiddenGroupStreamDisconnectTimer();
    const delayMs = getGroupStreamsHiddenDisconnectDelayMs(document.hidden);
    if (delayMs == null) return;
    hiddenDisconnectTimerRef.current = window.setTimeout(() => {
      hiddenDisconnectTimerRef.current = null;
      if (!document.hidden) return;
      disconnectGroupStreams({ resetConnected: false });
    }, delayMs);
  }

  function connectStream(groupId: string) {
    if (shouldStartGroupStreams(document.hidden)) {
      clearHiddenGroupStreamDisconnectTimer();
    }
    if (headlessReconnectTimerRef.current) {
      window.clearTimeout(headlessReconnectTimerRef.current);
      headlessReconnectTimerRef.current = null;
    }
    closeLedgerStream();
    closeHeadlessStream();

    if (!shouldStartGroupStreams(document.hidden)) {
      needsVisibilityCatchupRef.current = true;
      setSSEStatus("disconnected");
      return;
    }

    setSSEStatus("connecting");
    const es = new EventSource(
      api.withAuthToken(`/api/v1/groups/${encodeURIComponent(groupId)}/ledger/stream`),
    );
    const ledgerToken = sseRegistryRef.current.set("ledger", groupId, es);

    const isReconnect = hasConnectedOnceRef.current || needsVisibilityCatchupRef.current;

    es.onopen = () => {
      if (!sseRegistryRef.current.isCurrent(ledgerToken)) return;
      setSSEStatus("connected");
      hasConnectedOnceRef.current = true;

      // New SSE connections start at EOF, so every reconnect needs a
      // lightweight catch-up to cover the disconnect window.
      if (isReconnect) {
        needsVisibilityCatchupRef.current = false;
        void resyncAfterReconnect(groupId);
      }
    };

    es.onerror = () => {
      if (!sseRegistryRef.current.isCurrent(ledgerToken)) return;
      setSSEStatus("disconnected");
      // Keep this EventSource alive: native reconnect carries Last-Event-ID,
      // allowing the Rust stream to replay only the missed ledger events.
    };

    es.addEventListener("ledger", (e) => {
      if (!sseRegistryRef.current.isCurrent(ledgerToken)) return;
      const msg = e as MessageEvent;
      try {
        processLedgerEvent(groupId, JSON.parse(String(msg.data || "{}")) as LedgerEvent, {
          actors: actorsRef.current,
          activeTab: activeTabRef.current,
          chatAtBottom: chatAtBottomRef.current,
          onContextSync: () => {
            contextRefreshTimerRef.current = scheduleContextSummaryCatchup(groupId, {
              invalidateContextRead: api.invalidateContextRead,
              existingTimer: contextRefreshTimerRef.current,
              clearTimer: window.clearTimeout,
              setTimer: (callback, delayMs) => window.setTimeout(callback, delayMs),
              fetchContextSummary: (gid, options) => void fetchContext(gid, options),
            });
          },
          appendEvent,
          updateReadStatus,
          updateAckStatus,
          updateReplyStatus,
          incrementActorUnread,
          updateActorActivity,
          updateGroupRuntimeState,
          promoteStreamingEventsByPrefix,
          removeStreamingEvent,
          clearEmptyStreamingEventsForActor,
          refreshActors,
          refreshPresentation,
          incrementChatUnread,
          markPresentationSlotAttention,
          clearPresentationSlotAttention,
        });
      } catch {
        /* ignore parse errors */
      }
    });
    eventSourceRef.current = es;

    void hydrateHeadlessSnapshot(groupId)
      .catch(() => {
        /* ignore snapshot hydration failures */
      })
      .finally(() => {
        if (selectedGroupIdRef.current === groupId) {
          connectHeadlessStream(groupId, { replay: false });
        }
      });
  }

  function disconnectGroupStreams(options?: { resetConnected?: boolean }) {
    clearHiddenGroupStreamDisconnectTimer();
    if (headlessReconnectTimerRef.current) {
      window.clearTimeout(headlessReconnectTimerRef.current);
      headlessReconnectTimerRef.current = null;
    }
    closeLedgerStream();
    closeHeadlessStream();
    const flushBeforeClearing = options?.resetConnected === false;
    if (pendingHeadlessMessageFlushRef.current != null) {
      window.cancelAnimationFrame(pendingHeadlessMessageFlushRef.current);
      pendingHeadlessMessageFlushRef.current = null;
    }
    if (pendingHeadlessActivityFlushRef.current != null) {
      window.cancelAnimationFrame(pendingHeadlessActivityFlushRef.current);
      pendingHeadlessActivityFlushRef.current = null;
    }
    if (flushBeforeClearing) {
      flushPendingHeadlessActivities();
      flushPendingHeadlessMessages();
    }
    pendingHeadlessMessagesRef.current.clear();
    pendingHeadlessActivitiesRef.current.clear();
    headlessThreadIdByActorRef.current.clear();
    if (contextRefreshTimerRef.current) {
      window.clearTimeout(contextRefreshTimerRef.current);
      contextRefreshTimerRef.current = null;
    }
    headlessReconnectDelayRef.current = 1000;
    if (options?.resetConnected !== false) {
      hasConnectedOnceRef.current = false;
      needsVisibilityCatchupRef.current = false;
    } else {
      needsVisibilityCatchupRef.current = true;
    }
    setSSEStatus("disconnected");
  }

  useEffect(() => {
    function handleVisibilityChange() {
      if (!shouldStartGroupStreams(document.hidden)) {
        scheduleHiddenGroupStreamDisconnect();
        return;
      }
      clearHiddenGroupStreamDisconnectTimer();
      const gid = String(selectedGroupIdRef.current || "").trim();
      if (!gid) return;
      if (!eventSourceRef.current) {
        connectStream(gid);
        return;
      }
      if (!headlessEventSourceRef.current) {
        void hydrateHeadlessSnapshot(gid)
          .catch(() => {
            /* ignore snapshot hydration failures */
          })
          .finally(() => {
            if (selectedGroupIdRef.current === gid && !headlessEventSourceRef.current) {
              connectHeadlessStream(gid, { replay: false });
            }
          });
      }
    }

    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- uses refs and stable store actions; selected group changes are handled by App lifecycle.
  }, []);

  function cleanup() {
    disconnectGroupStreams({ resetConnected: true });
  }

  return { connectStream, fetchContext, cleanup, contextRefreshTimerRef };
}
