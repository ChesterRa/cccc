// Group state store (groups, actors, events, context, settings).
import { create } from "zustand";
import type {
  GroupMeta,
  GroupDoc,
  GroupRuntimeStatus,
  HeadlessStreamEvent,
} from "../types";
import {
  normalizeReplySessionTimestamp,
  upsertReplySession,
} from "./chatStreamingSessions";
import {
  buildChatBucketPatch,
  buildPrimedGroupState,
  ensureGroupChatBucket,
  getCachedGroupView,
  getGroupChatBucket,
  GroupChatBucket,
  HEADLESS_RAW_EVENT_LIMIT,
  loadArchivedGroupIds,
  loadGroupOrder,
  loadSelectedGroupId,
  MAX_UI_EVENTS,
  mergeArchivedGroupIds,
  mergeGroupOrder,
  mergeLedgerEventStatuses,
  normalizeGroupIdList,
  patchGroupRuntimeStatus,
  presentationRequestEpochByGroup,
  reorderGroupSubset,
  resolveChatGroupId,
  saveArchivedGroupIds,
  saveCurrentViewSnapshot,
  saveGroupOrder,
  saveGroupView,
  saveSelectedGroupId,
  beginGroupRequestEpoch,
  settingsRequestEpochByGroup,
  updateAckAtIndex,
  updateReadThroughIndex,
  updateReplyAtIndex,
} from "./groupStoreCore";
import { createGroupStoreAsyncActions } from "./groupStoreAsyncActions";
import type { GroupState } from "./groupStoreTypes";
import {
  clearEmptyStreamingEventsForActorPatch,
  clearStreamingEventsForActorPatch,
  clearStreamingPlaceholderPatch,
  clearTransientStreamingEventsForActorPatch,
  completeStreamingEventsForActorPatch,
  promoteStreamingEventToStreamPatch,
  promoteStreamingEventsByPrefixPatch,
  reconcileStreamingMessagePatch,
  removeStreamingEventPatch,
  removeStreamingEventsByPrefixPatch,
  upsertStreamingActivitiesPatch,
  upsertStreamingActivityPatch,
  upsertStreamingEventPatch,
  upsertStreamingTextPatch,
} from "./groupStreamingReducers";
import { computeGroupRuntimePatch } from "../utils/groupRuntimeProjection";

function stableSerialize(value: unknown): string {
  if (value === null || value === undefined) return String(value);
  if (typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map((item) => stableSerialize(item)).join(",")}]`;
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record).sort().map((key) => `${JSON.stringify(key)}:${stableSerialize(record[key])}`).join(",")}}`;
}

function buildHeadlessEventSignature(event: HeadlessStreamEvent): string {
  return [
    String(event.id || "").trim(),
    String(event.ts || "").trim(),
    String(event.actor_id || "").trim(),
    String(event.type || "").trim(),
    stableSerialize(event.data || {}),
  ].join("|");
}

export const useGroupStore = create<GroupState>((set, get) => ({
  // Initial state
  groups: [],
  groupOrder: loadGroupOrder(),
  archivedGroupIds: loadArchivedGroupIds(),
  selectedGroupId: loadSelectedGroupId(),
  chatByGroup: {},
  groupDoc: null,
  events: [],
  chatWindow: null,
  actors: [],
  internalRuntimeActorsByGroup: {},
  groupContext: null,
  groupSettings: null,
  groupPresentation: null,
  runtimes: [],
  selectedGroupActorsHydrating: false,
  hasMoreHistory: true,
  isLoadingHistory: false,
  isChatWindowLoading: false,

  // Sync actions
  setGroups: (groups) => {
    const storedOrder = get().groupOrder;
    const storedArchived = get().archivedGroupIds;
    const mergedOrder = mergeGroupOrder(storedOrder, groups);
    const mergedArchived = mergeArchivedGroupIds(storedArchived, groups);
    saveGroupOrder(mergedOrder);
    saveArchivedGroupIds(mergedArchived);
    set({ groups, groupOrder: mergedOrder, archivedGroupIds: mergedArchived });
  },
  setGroupOrder: (order) => {
    saveGroupOrder(order);
    set({ groupOrder: order });
  },
  reorderGroupsInSection: (section, fromIndex, toIndex) => {
    const { groupOrder, archivedGroupIds } = get();
    const archivedSet = new Set(archivedGroupIds);
    const subsetIds = groupOrder.filter((id) =>
      section === "archived" ? archivedSet.has(id) : !archivedSet.has(id)
    );
    const nextOrder = reorderGroupSubset(groupOrder, subsetIds, fromIndex, toIndex);
    if (nextOrder === groupOrder) return;
    saveGroupOrder(nextOrder);
    set({ groupOrder: nextOrder });
  },
  archiveGroup: (groupId) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    const next = normalizeGroupIdList([...get().archivedGroupIds, gid]);
    saveArchivedGroupIds(next);
    set({ archivedGroupIds: next });
  },
  restoreGroup: (groupId) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    const next = get().archivedGroupIds.filter((id) => id !== gid);
    saveArchivedGroupIds(next);
    set({ archivedGroupIds: next });
  },
  getOrderedGroups: () => {
    const { groups, groupOrder, selectedGroupId, groupDoc, actors } = get();
    const groupMap = new Map(groups.map((g) => [String(g.group_id || ""), g]));
    const ordered: GroupMeta[] = [];
    for (const id of groupOrder) {
      const g = groupMap.get(id);
      if (g) ordered.push(g);
    }
    // Include any groups not in order (shouldn't happen normally, but be safe)
    for (const g of groups) {
      const id = String(g.group_id || "");
      if (!groupOrder.includes(id)) ordered.push(g);
    }
    return ordered.map((group) => {
      const gid = String(group.group_id || "").trim();
      const isSelected = gid === String(selectedGroupId || "").trim();
      const cached = isSelected ? null : getCachedGroupView(gid);
      const patch = computeGroupRuntimePatch({
        group,
        groupDoc: isSelected ? groupDoc : null,
        actors: isSelected ? actors : (cached?.actors || []),
      });
      return {
        ...group,
        ...patch,
      };
    });
  },
  setSelectedGroupId: (id) => {
    const gid = String(id || "").trim();
    saveSelectedGroupId(gid);
    set((state) => {
      const prevGid = String(state.selectedGroupId || "").trim();

      // 切组前先把当前视图快照落到缓存，回切时才能做到秒开。
      if (prevGid && prevGid !== gid) {
        saveCurrentViewSnapshot(prevGid, state);
      }

      const nextChatByGroup = ensureGroupChatBucket(state.chatByGroup, gid);
      return {
        selectedGroupId: gid,
        chatByGroup: nextChatByGroup,
        selectedGroupActorsHydrating: !!gid,
        ...buildPrimedGroupState(gid, state.groups),
      };
    });
  },
  setGroupDoc: (doc) => set({ groupDoc: doc }),
  updateGroupRuntimeState: (groupId, patch) =>
    set((state) => {
      const gid = String(groupId || "").trim();
      if (!gid) return state;
      const currentGroup = state.groups.find((group) => String(group.group_id || "").trim() === gid) || null;
      const currentRuntime = currentGroup?.runtime_status
        || (state.groupDoc?.group_id === gid ? state.groupDoc.runtime_status : null)
        || null;
      const nextRuntime: GroupRuntimeStatus = {
        lifecycle_state: String(patch.lifecycle_state || currentRuntime?.lifecycle_state || currentGroup?.state || "active"),
        runtime_running: patch.runtime_running ?? currentRuntime?.runtime_running ?? currentGroup?.running ?? false,
        running_actor_count: Number.isFinite(Number(patch.running_actor_count))
          ? Number(patch.running_actor_count)
          : Number(currentRuntime?.running_actor_count || 0),
        has_running_foreman: patch.has_running_foreman ?? currentRuntime?.has_running_foreman ?? false,
      };
      const nextGroups = patchGroupRuntimeStatus(state.groups, gid, nextRuntime);
      const nextGroupDoc =
        state.groupDoc && String(state.groupDoc.group_id || "").trim() === gid
          ? {
              ...state.groupDoc,
              running: nextRuntime.runtime_running,
              state: nextRuntime.lifecycle_state as GroupDoc["state"],
              runtime_status: nextRuntime,
            }
          : state.groupDoc;
      saveGroupView(gid, { groupDoc: nextGroupDoc });
      if (nextGroups === state.groups && nextGroupDoc === state.groupDoc) {
        return state;
      }
      return {
        groups: nextGroups,
        ...(nextGroupDoc !== state.groupDoc ? { groupDoc: nextGroupDoc } : {}),
      };
    }),
  setEvents: (events, groupId) =>
    set((state) => buildChatBucketPatch(state, resolveChatGroupId(state, groupId), { events }) ?? state),
  mergeEventStatuses: (statuses, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const nextEvents = mergeLedgerEventStatuses(bucket.events, statuses);
      const nextChatWindow = bucket.chatWindow
        ? {
            ...bucket.chatWindow,
            events: mergeLedgerEventStatuses(bucket.chatWindow.events, statuses),
          }
        : bucket.chatWindow;
      if (nextEvents === bucket.events && nextChatWindow === bucket.chatWindow) return state;
      return buildChatBucketPatch(state, gid, { events: nextEvents, chatWindow: nextChatWindow }) ?? state;
    }),
  setChatWindow: (chatWindow, groupId) =>
    set((state) => buildChatBucketPatch(state, resolveChatGroupId(state, groupId), { chatWindow }) ?? state),
  appendEvent: (event, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      if (event.id && bucket.events.some((e) => e.id === event.id)) return state;
      const nextEvents = bucket.events.concat([event]);
      const patch: Partial<GroupChatBucket> = {
        events: nextEvents.length > MAX_UI_EVENTS ? nextEvents.slice(nextEvents.length - MAX_UI_EVENTS) : nextEvents,
      };
      if (String(event.kind || "").trim() === "chat.message" && String(event.by || "").trim() !== "user") {
        const data = event.data && typeof event.data === "object"
          ? event.data as { pending_event_id?: unknown; reply_to?: unknown; text?: unknown; activities?: unknown; stream_id?: unknown }
          : {};
        const pendingEventId = String(data.pending_event_id || data.reply_to || "").trim();
        const actorId = String(event.by || "").trim();
        if (pendingEventId && actorId) {
          const { replySessionsByPendingEventId, pendingEventIdByStreamId } = upsertReplySession(
            bucket.replySessionsByPendingEventId,
            bucket.pendingEventIdByStreamId,
            {
              pendingEventId,
              actorId,
              streamId: String(data.stream_id || "").trim() || undefined,
              phase: "completed",
              canonicalEventId: String(event.id || "").trim() || undefined,
              updatedAt: normalizeReplySessionTimestamp(String(event.ts || "")),
            },
          );
          patch.replySessionsByPendingEventId = replySessionsByPendingEventId;
          patch.pendingEventIdByStreamId = pendingEventIdByStreamId;
        }
      }
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  appendHeadlessEvent: (event, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const actorId = String(event.actor_id || "").trim();
      const eventType = String(event.type || "").trim();
      if (!actorId || !eventType) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const currentByActor = bucket.rawHeadlessEventsByActorId || {};
      const currentEvents = Array.isArray(currentByActor[actorId]) ? currentByActor[actorId] : [];
      const nextSignature = buildHeadlessEventSignature(event);
      const exists = currentEvents.some((candidate) => buildHeadlessEventSignature(candidate) === nextSignature);
      if (exists) return state;
      const nextEvents = currentEvents.concat([event]);
      return buildChatBucketPatch(state, gid, {
        rawHeadlessEventsByActorId: {
          ...currentByActor,
          [actorId]: nextEvents.length > HEADLESS_RAW_EVENT_LIMIT
            ? nextEvents.slice(nextEvents.length - HEADLESS_RAW_EVENT_LIMIT)
            : nextEvents,
        },
      }) ?? state;
    }),
  upsertStreamingEvent: (event, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = upsertStreamingEventPatch(bucket, gid, event);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  upsertStreamingText: (streamId, text, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = upsertStreamingTextPatch(bucket, streamId, text);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  upsertStreamingActivities: (streamId, activities, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = upsertStreamingActivitiesPatch(bucket, streamId, activities);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  upsertStreamingActivity: (actorId, match, activity, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = upsertStreamingActivityPatch(bucket, gid, actorId, match, activity);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  removeStreamingEvent: (streamId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = removeStreamingEventPatch(bucket, streamId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  removeStreamingEventsByPrefix: (streamIdPrefix, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = removeStreamingEventsByPrefixPatch(bucket, streamIdPrefix);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  promoteStreamingEventsByPrefix: (streamIdPrefix, pendingEventId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = promoteStreamingEventsByPrefixPatch(bucket, streamIdPrefix, pendingEventId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  promoteStreamingEventToStream: (actorId, pendingEventId, streamId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = promoteStreamingEventToStreamPatch(bucket, actorId, pendingEventId, streamId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  reconcileStreamingMessage: ({
    actorId,
    pendingEventId,
    streamId,
    ts,
    fullText,
    eventText,
    activities,
    completed,
    transientStream,
    phase,
    groupId,
  }) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = reconcileStreamingMessagePatch(bucket, gid, actorId, {
        pendingEventId,
        streamId,
        ts,
        fullText,
        eventText,
        activities,
        completed,
        transientStream,
        phase,
      });
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  completeStreamingEventsForActor: (actorId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = completeStreamingEventsForActorPatch(bucket, actorId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  clearStreamingEventsForActor: (actorId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = clearStreamingEventsForActorPatch(bucket, actorId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  clearEmptyStreamingEventsForActor: (actorId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = clearEmptyStreamingEventsForActorPatch(bucket, actorId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  clearTransientStreamingEventsForActor: (actorId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = clearTransientStreamingEventsForActorPatch(bucket, actorId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  clearStreamingPlaceholder: (actorId, pendingEventId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const patch = clearStreamingPlaceholderPatch(bucket, actorId, pendingEventId);
      if (!patch) return state;
      return buildChatBucketPatch(state, gid, patch) ?? state;
    }),
  prependEvents: (newEvents, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const existingIds = new Set(bucket.events.map((event) => event.id).filter(Boolean));
      const uniqueNew = newEvents.filter((event) => event.id && !existingIds.has(event.id));
      const merged = [...uniqueNew, ...bucket.events];
      return buildChatBucketPatch(state, gid, {
        events: merged.length > MAX_UI_EVENTS ? merged.slice(0, MAX_UI_EVENTS) : merged,
      }) ?? state;
    }),
  setActors: (actors) =>
    set((state) => {
      const gid = String(state.selectedGroupId || "").trim();
      if (gid) {
        saveGroupView(gid, { actors });
      }
      return { actors };
    }),
  incrementActorUnread: (actorIds) =>
    set((state) => {
      if (!state.actors.length || !actorIds.length) return state;
      const targets = new Set(actorIds.map((id) => String(id || "").trim()).filter(Boolean));
      if (targets.size === 0) return state;

      let changed = false;
      const next = state.actors.map((actor) => {
        const actorId = String(actor.id || "").trim();
        if (!targets.has(actorId)) return actor;
        changed = true;
        return {
          ...actor,
          unread_count: Math.max(0, Number(actor.unread_count || 0)) + 1,
        };
      });

      if (!changed) return state;
      const gid = String(state.selectedGroupId || "").trim();
      if (gid) {
        saveGroupView(gid, { actors: next });
      }
      return { actors: next };
    }),
  updateActorActivity: (updates) =>
    set((state) => {
      if (!state.actors.length || !updates.length) return state;
      const map = new Map(updates.map((u) => [u.id, u]));
      let changed = false;
      const next = state.actors.map((a) => {
        const u = map.get(a.id);
        if (
          u && (
            a.idle_seconds !== (u.idle_seconds ?? null)
            || a.running !== u.running
            || a.effective_working_state !== u.effective_working_state
            || a.effective_working_reason !== u.effective_working_reason
            || a.effective_working_updated_at !== (u.effective_working_updated_at ?? null)
            || a.effective_active_task_id !== (u.effective_active_task_id ?? null)
          )
        ) {
          changed = true;
          return {
            ...a,
            idle_seconds: u.idle_seconds ?? null,
            running: u.running,
            effective_working_state: u.effective_working_state,
            effective_working_reason: u.effective_working_reason,
            effective_working_updated_at: u.effective_working_updated_at ?? null,
            effective_active_task_id: u.effective_active_task_id ?? null,
          };
        }
        return a;
      });
      if (!changed) return state;
      const gid = String(state.selectedGroupId || "").trim();
      if (gid) {
        saveGroupView(gid, { actors: next });
      }
      return { actors: next };
    }),
  setGroupContext: (ctx) =>
    set((state) => {
      const gid = String(state.selectedGroupId || "").trim();
      if (gid) {
        saveGroupView(gid, { groupContext: ctx });
      }
      return { groupContext: ctx };
    }),
  setGroupSettings: (settings) =>
    set((state) => {
      const gid = String(state.selectedGroupId || "").trim();
      if (gid) {
        beginGroupRequestEpoch(settingsRequestEpochByGroup, gid);
        saveGroupView(gid, { groupSettings: settings });
      }
      return { groupSettings: settings };
    }),
  setGroupPresentation: (presentation) =>
    set((state) => {
      const gid = String(state.selectedGroupId || "").trim();
      if (gid) {
        beginGroupRequestEpoch(presentationRequestEpochByGroup, gid);
        saveGroupView(gid, { groupPresentation: presentation });
      }
      return { groupPresentation: presentation };
    }),
  setRuntimes: (runtimes) => set({ runtimes }),

  updateReadStatus: (eventId, actorId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const eventIndex = bucket.events.findIndex(
        (event) => event.kind === "chat.message" && String(event.id || "") === eventId
      );
      if (eventIndex < 0 && !bucket.chatWindow) return state;

      const liveResult = eventIndex >= 0
        ? updateReadThroughIndex(bucket.events, eventIndex, actorId)
        : { next: bucket.events, changed: false };
      let nextWindow = bucket.chatWindow;
      let didChange = liveResult.changed;

      if (bucket.chatWindow) {
        const windowIndex = bucket.chatWindow.events.findIndex(
          (event) => event.kind === "chat.message" && String(event.id || "") === eventId
        );
        if (windowIndex >= 0) {
          const windowResult = updateReadThroughIndex(bucket.chatWindow.events, windowIndex, actorId);
          if (windowResult.changed) {
            nextWindow = { ...bucket.chatWindow, events: windowResult.next };
            didChange = true;
          }
        }
      }

      if (!didChange) return state;
      return buildChatBucketPatch(state, gid, {
        events: liveResult.next,
        chatWindow: nextWindow,
      }) ?? state;
    }),

  updateAckStatus: (eventId, actorId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const eventIndex = bucket.events.findIndex(
        (event) => event.kind === "chat.message" && String(event.id || "") === eventId
      );
      if (eventIndex < 0 && !bucket.chatWindow) return state;

      const liveResult = eventIndex >= 0
        ? updateAckAtIndex(bucket.events, eventIndex, actorId)
        : { next: bucket.events, changed: false };
      let nextWindow = bucket.chatWindow;
      let didChange = liveResult.changed;

      if (bucket.chatWindow) {
        const windowIndex = bucket.chatWindow.events.findIndex(
          (event) => event.kind === "chat.message" && String(event.id || "") === eventId
        );
        if (windowIndex >= 0) {
          const windowResult = updateAckAtIndex(bucket.chatWindow.events, windowIndex, actorId);
          if (windowResult.changed) {
            nextWindow = { ...bucket.chatWindow, events: windowResult.next };
            didChange = true;
          }
        }
      }

      if (!didChange) return state;
      return buildChatBucketPatch(state, gid, {
        events: liveResult.next,
        chatWindow: nextWindow,
      }) ?? state;
    }),

  updateReplyStatus: (eventId, actorId, groupId) =>
    set((state) => {
      const gid = resolveChatGroupId(state, groupId);
      if (!gid) return state;
      const bucket = getGroupChatBucket(state.chatByGroup, gid);
      const eventIndex = bucket.events.findIndex(
        (event) => event.kind === "chat.message" && String(event.id || "") === eventId
      );
      if (eventIndex < 0 && !bucket.chatWindow) return state;

      const liveResult = eventIndex >= 0
        ? updateReplyAtIndex(bucket.events, eventIndex, actorId)
        : { next: bucket.events, changed: false };
      let nextWindow = bucket.chatWindow;
      let didChange = liveResult.changed;

      if (bucket.chatWindow) {
        const windowIndex = bucket.chatWindow.events.findIndex(
          (event) => event.kind === "chat.message" && String(event.id || "") === eventId
        );
        if (windowIndex >= 0) {
          const windowResult = updateReplyAtIndex(bucket.chatWindow.events, windowIndex, actorId);
          if (windowResult.changed) {
            nextWindow = { ...bucket.chatWindow, events: windowResult.next };
            didChange = true;
          }
        }
      }

      if (!didChange) return state;
      return buildChatBucketPatch(state, gid, {
        events: liveResult.next,
        chatWindow: nextWindow,
      }) ?? state;
    }),
  setHasMoreHistory: (value, groupId) =>
    set((state) => buildChatBucketPatch(state, resolveChatGroupId(state, groupId), { hasMoreHistory: value }) ?? state),
  setIsLoadingHistory: (value, groupId) =>
    set((state) => buildChatBucketPatch(state, resolveChatGroupId(state, groupId), { isLoadingHistory: value }) ?? state),
  setIsChatWindowLoading: (value, groupId) =>
    set((state) => buildChatBucketPatch(state, resolveChatGroupId(state, groupId), { isChatWindowLoading: value }) ?? state),
  ...createGroupStoreAsyncActions(set, get),
}));
