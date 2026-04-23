import type {
  Actor,
  GroupContext,
  GroupDoc,
  GroupMeta,
  GroupPresentation,
  GroupRuntimeStatus,
  GroupSettings,
  HeadlessStreamEvent,
  HeadlessPreviewBlock,
  HeadlessPreviewSession,
  LedgerEvent,
  LedgerEventStatusPayload,
  StreamingActivity,
} from "../types";
import { dedupeStreamingActivities, type StreamingReplySession } from "./chatStreamingSessions";

export type ChatWindowState = {
  groupId: string;
  centerEventId: string;
  centerIndex: number;
  events: LedgerEvent[];
  hasMoreBefore: boolean;
  hasMoreAfter: boolean;
};

export type GroupChatBucket = {
  events: LedgerEvent[];
  streamingEvents: LedgerEvent[];
  streamingTextByStreamId: Record<string, string>;
  streamingActivitiesByStreamId: Record<string, StreamingActivity[]>;
  replySessionsByPendingEventId: Record<string, StreamingReplySession>;
  pendingEventIdByStreamId: Record<string, string>;
  rawHeadlessEventsByActorId: Record<string, HeadlessStreamEvent[]>;
  previewSessionsByActorId: Record<string, HeadlessPreviewSession[]>;
  latestActorPreviewByActorId: Record<string, HeadlessPreviewSession>;
  latestActorTextByActorId: Record<string, string>;
  latestActorActivitiesByActorId: Record<string, StreamingActivity[]>;
  chatWindow: ChatWindowState | null;
  hasMoreHistory: boolean;
  hasLoadedTail: boolean;
  isLoadingHistory: boolean;
  isChatWindowLoading: boolean;
};

export type GroupViewSnapshot = {
  groupDoc: GroupDoc | null;
  events: LedgerEvent[];
  actors: Actor[];
  groupContext: GroupContext | null;
  groupSettings: GroupSettings | null;
  groupPresentation: GroupPresentation | null;
  hasMoreHistory: boolean;
  cachedAt: number;
};

export type GroupStateSnapshot = {
  groups: GroupMeta[];
  selectedGroupId: string;
  chatByGroup: Record<string, GroupChatBucket>;
  groupDoc: GroupDoc | null;
  actors: Actor[];
  groupContext: GroupContext | null;
  groupSettings: GroupSettings | null;
  groupPresentation: GroupPresentation | null;
};

export const EMPTY_CHAT_BUCKET: GroupChatBucket = {
  events: [],
  streamingEvents: [],
  streamingTextByStreamId: {},
  streamingActivitiesByStreamId: {},
  replySessionsByPendingEventId: {},
  pendingEventIdByStreamId: {},
  rawHeadlessEventsByActorId: {},
  previewSessionsByActorId: {},
  latestActorPreviewByActorId: {},
  latestActorTextByActorId: {},
  latestActorActivitiesByActorId: {},
  chatWindow: null,
  hasMoreHistory: false,
  hasLoadedTail: false,
  isLoadingHistory: false,
  isChatWindowLoading: false,
};

export const INITIAL_LEDGER_TAIL_LIMIT = 60;
export const MAX_UI_EVENTS = 800;
const GROUP_VIEW_CACHE_TTL_MS = 300_000;
const GROUP_ORDER_KEY = "cccc-group-order";
const ARCHIVED_GROUP_IDS_KEY = "cccc-archived-group-ids";
const SELECTED_GROUP_ID_KEY = "cccc-selected-group-id";
const RUNTIME_ONLY_UI_EVENT_KINDS = new Set(["actor.activity", "context.sync"]);
export const HEADLESS_RAW_EVENT_LIMIT = 400;

export let refreshGroupsInFlight = false;
export let refreshGroupsQueued = false;
export const refreshActorsInFlight = new Set<string>();
export const refreshActorsQueued = new Map<string, boolean>();
export const warmGroupInFlight = new Set<string>();
export const loadGroupInFlight = new Map<string, Promise<void>>();
export let loadGroupToken = 0;
export const deferredUnreadRefreshTimers = new Map<string, ReturnType<typeof globalThis.setTimeout>>();
export const settingsRequestEpochByGroup = new Map<string, number>();
export const presentationRequestEpochByGroup = new Map<string, number>();
export const chatWindowRequestEpochByGroup = new Map<string, number>();
export const internalActorsRequestEpochByGroup = new Map<string, number>();
export const contextRequestEpochByGroup = new Map<string, number>();
export const groupViewCache = new Map<string, GroupViewSnapshot>();

export function setRefreshGroupsInFlight(value: boolean): void {
  refreshGroupsInFlight = value;
}

export function setRefreshGroupsQueued(value: boolean): void {
  refreshGroupsQueued = value;
}

export function incrementLoadGroupToken(): number {
  loadGroupToken += 1;
  return loadGroupToken;
}

export function normalizeGroupIdList(ids: string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const raw of ids) {
    const id = String(raw || "").trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}

export function loadGroupOrder(): string[] {
  try {
    const stored = localStorage.getItem(GROUP_ORDER_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      if (Array.isArray(parsed)) return normalizeGroupIdList(parsed);
    }
  } catch (e) {
    console.warn("Failed to read group order from localStorage:", e);
  }
  return [];
}

export function loadArchivedGroupIds(): string[] {
  try {
    const stored = localStorage.getItem(ARCHIVED_GROUP_IDS_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      if (Array.isArray(parsed)) return normalizeGroupIdList(parsed);
    }
  } catch (e) {
    console.warn("Failed to read archived groups from localStorage:", e);
  }
  return [];
}

export function saveGroupOrder(order: string[]): void {
  try {
    localStorage.setItem(GROUP_ORDER_KEY, JSON.stringify(normalizeGroupIdList(order)));
  } catch (e) {
    console.warn("Failed to persist group order to localStorage:", e);
  }
}

export function saveArchivedGroupIds(groupIds: string[]): void {
  try {
    const next = normalizeGroupIdList(groupIds);
    if (next.length === 0) {
      localStorage.removeItem(ARCHIVED_GROUP_IDS_KEY);
      return;
    }
    localStorage.setItem(ARCHIVED_GROUP_IDS_KEY, JSON.stringify(next));
  } catch (e) {
    console.warn("Failed to persist archived groups to localStorage:", e);
  }
}

export function loadSelectedGroupId(): string {
  try {
    return String(localStorage.getItem(SELECTED_GROUP_ID_KEY) || "").trim();
  } catch (e) {
    console.warn("Failed to read selected group from localStorage:", e);
  }
  return "";
}

export function saveSelectedGroupId(groupId: string): void {
  try {
    const gid = String(groupId || "").trim();
    if (!gid) {
      localStorage.removeItem(SELECTED_GROUP_ID_KEY);
      return;
    }
    localStorage.setItem(SELECTED_GROUP_ID_KEY, gid);
  } catch (e) {
    console.warn("Failed to persist selected group to localStorage:", e);
  }
}

export function mergeGroupOrder(storedOrder: string[], groups: GroupMeta[]): string[] {
  const currentIds = new Set(groups.map((g) => String(g.group_id || "")));
  const validOrder = storedOrder.filter((id) => currentIds.has(id));
  const orderedSet = new Set(validOrder);
  const newIds = groups
    .map((g) => String(g.group_id || ""))
    .filter((id) => id && !orderedSet.has(id));
  return [...validOrder, ...newIds];
}

export function mergeArchivedGroupIds(storedIds: string[], groups: GroupMeta[]): string[] {
  const currentIds = new Set(groups.map((g) => String(g.group_id || "").trim()).filter(Boolean));
  return normalizeGroupIdList(storedIds).filter((id) => currentIds.has(id));
}

export function reorderGroupSubset(globalOrder: string[], subsetIds: string[], fromIndex: number, toIndex: number): string[] {
  if (fromIndex === toIndex) return globalOrder;
  if (fromIndex < 0 || toIndex < 0) return globalOrder;
  if (fromIndex >= subsetIds.length || toIndex >= subsetIds.length) return globalOrder;

  const nextSubset = subsetIds.slice();
  const [moved] = nextSubset.splice(fromIndex, 1);
  if (!moved) return globalOrder;
  nextSubset.splice(toIndex, 0, moved);

  const subsetSet = new Set(subsetIds);
  let cursor = 0;
  return globalOrder.map((id) => {
    if (!subsetSet.has(id)) return id;
    const replacement = nextSubset[cursor];
    cursor += 1;
    return replacement || id;
  });
}

export function beginGroupRequestEpoch(map: Map<string, number>, groupId: string): number {
  const gid = String(groupId || "").trim();
  if (!gid) return 0;
  const next = Number(map.get(gid) || 0) + 1;
  map.set(gid, next);
  return next;
}

export function isLatestGroupRequestEpoch(map: Map<string, number>, groupId: string, epoch: number): boolean {
  const gid = String(groupId || "").trim();
  if (!gid || epoch <= 0) return false;
  return Number(map.get(gid) || 0) === epoch;
}

export function beginContextRequest(groupId: string): number {
  return beginGroupRequestEpoch(contextRequestEpochByGroup, groupId);
}

export function isLatestContextRequest(groupId: string, epoch: number): boolean {
  return isLatestGroupRequestEpoch(contextRequestEpochByGroup, groupId, epoch);
}

function cloneGroupDoc(doc: GroupDoc | null | undefined): GroupDoc | null {
  return doc ? { ...doc } : null;
}

function cloneEvents(events: LedgerEvent[] | undefined): LedgerEvent[] {
  return Array.isArray(events) ? [...events] : [];
}

function cloneActors(actors: Actor[] | undefined): Actor[] {
  return Array.isArray(actors) ? [...actors] : [];
}

function cloneGroupContext(ctx: GroupContext | null | undefined): GroupContext | null {
  return ctx ? { ...ctx } : null;
}

function cloneGroupSettings(settings: GroupSettings | null | undefined): GroupSettings | null {
  return settings ? { ...settings } : null;
}

function cloneGroupPresentation(presentation: GroupPresentation | null | undefined): GroupPresentation | null {
  if (!presentation) return null;
  return {
    ...presentation,
    slots: Array.isArray(presentation.slots)
      ? presentation.slots.map((slot) => ({
        ...slot,
        card: slot.card
          ? {
            ...slot.card,
            content: {
              ...(slot.card.content || {}),
              table: slot.card.content?.table
                ? {
                  ...slot.card.content.table,
                  columns: [...(slot.card.content.table.columns || [])],
                  rows: Array.isArray(slot.card.content.table.rows)
                    ? slot.card.content.table.rows.map((row) => [...row])
                    : [],
                }
                : null,
            },
          }
          : null,
      }))
      : [],
  };
}

export function getCachedGroupView(groupId: string): GroupViewSnapshot | null {
  const gid = String(groupId || "").trim();
  if (!gid) return null;
  const cached = groupViewCache.get(gid);
  if (!cached) return null;
  if (Date.now() - cached.cachedAt > GROUP_VIEW_CACHE_TTL_MS) return null;
  return {
    groupDoc: cloneGroupDoc(cached.groupDoc),
    events: cloneEvents(cached.events),
    actors: cloneActors(cached.actors),
    groupContext: cloneGroupContext(cached.groupContext),
    groupSettings: cloneGroupSettings(cached.groupSettings),
    groupPresentation: cloneGroupPresentation(cached.groupPresentation),
    hasMoreHistory: !!cached.hasMoreHistory,
    cachedAt: cached.cachedAt,
  };
}

export function saveGroupView(groupId: string, patch: Partial<Omit<GroupViewSnapshot, "cachedAt">>): void {
  const gid = String(groupId || "").trim();
  if (!gid) return;

  const prev = groupViewCache.get(gid);
  groupViewCache.set(gid, {
    groupDoc: patch.groupDoc !== undefined ? cloneGroupDoc(patch.groupDoc) : cloneGroupDoc(prev?.groupDoc),
    events: patch.events !== undefined ? cloneEvents(patch.events) : cloneEvents(prev?.events),
    actors: patch.actors !== undefined ? cloneActors(patch.actors) : cloneActors(prev?.actors),
    groupContext: patch.groupContext !== undefined ? cloneGroupContext(patch.groupContext) : cloneGroupContext(prev?.groupContext),
    groupSettings: patch.groupSettings !== undefined ? cloneGroupSettings(patch.groupSettings) : cloneGroupSettings(prev?.groupSettings),
    groupPresentation: patch.groupPresentation !== undefined ? cloneGroupPresentation(patch.groupPresentation) : cloneGroupPresentation(prev?.groupPresentation),
    hasMoreHistory: patch.hasMoreHistory !== undefined ? !!patch.hasMoreHistory : !!prev?.hasMoreHistory,
    cachedAt: Date.now(),
  });
}

export function clearDeferredUnreadRefresh(groupId: string): void {
  const gid = String(groupId || "").trim();
  const timer = deferredUnreadRefreshTimers.get(gid);
  if (timer !== undefined) {
    globalThis.clearTimeout(timer);
    deferredUnreadRefreshTimers.delete(gid);
  }
}

export function scheduleDeferredUnreadRefresh(groupId: string, task: () => void): void {
  const gid = String(groupId || "").trim();
  if (!gid) return;
  clearDeferredUnreadRefresh(gid);
  const timer = globalThis.setTimeout(() => {
    deferredUnreadRefreshTimers.delete(gid);
    task();
  }, 0);
  deferredUnreadRefreshTimers.set(gid, timer);
}

export function mergeActorUnreadCounts(nextActors: Actor[], previousActors: Actor[]): Actor[] {
  if (!nextActors.length || !previousActors.length) return nextActors;
  const unreadByActorId = new Map(
    previousActors
      .filter((actor) => typeof actor?.unread_count === "number")
      .map((actor) => [String(actor.id || ""), Number(actor.unread_count || 0)] as const),
  );

  return nextActors.map((actor) => {
    if (typeof actor?.unread_count === "number") return actor;
    const actorId = String(actor.id || "");
    if (!actorId || !unreadByActorId.has(actorId)) return actor;
    return { ...actor, unread_count: unreadByActorId.get(actorId) ?? 0 };
  });
}

export function saveCurrentViewSnapshot(groupId: string, state: GroupStateSnapshot): void {
  const gid = String(groupId || "").trim();
  if (!gid) return;

  const bucket = state.chatByGroup[gid] || EMPTY_CHAT_BUCKET;
  const shellDoc = buildShellGroupDoc(gid, state.groups, null);
  saveGroupView(gid, {
    groupDoc: state.groupDoc && String(state.groupDoc.group_id || "") === gid ? state.groupDoc : shellDoc,
    events: bucket.events,
    actors: state.actors,
    groupContext: state.groupContext,
    groupSettings: state.groupSettings,
    groupPresentation: state.groupPresentation,
    hasMoreHistory: bucket.hasMoreHistory,
  });
}

export function getGroupChatBucket(chatByGroup: Record<string, GroupChatBucket>, groupId: string): GroupChatBucket {
  const gid = String(groupId || "").trim();
  if (!gid) return EMPTY_CHAT_BUCKET;
  const stored = chatByGroup[gid];
  if (stored) {
    return {
      ...stored,
      streamingTextByStreamId: stored.streamingTextByStreamId || {},
      streamingActivitiesByStreamId: stored.streamingActivitiesByStreamId || {},
      replySessionsByPendingEventId: stored.replySessionsByPendingEventId || {},
      pendingEventIdByStreamId: stored.pendingEventIdByStreamId || {},
      rawHeadlessEventsByActorId: stored.rawHeadlessEventsByActorId || {},
      previewSessionsByActorId: stored.previewSessionsByActorId || Object.fromEntries(
        Object.entries(stored.latestActorPreviewByActorId || {}).map(([actorId, preview]) => [actorId, [preview]])
      ),
      latestActorPreviewByActorId: stored.latestActorPreviewByActorId || {},
      latestActorTextByActorId: stored.latestActorTextByActorId || {},
      latestActorActivitiesByActorId: stored.latestActorActivitiesByActorId || {},
    };
  }
  const cached = getCachedGroupView(gid);
  return {
    events: cached?.events ? [...cached.events] : [],
    streamingEvents: [],
    streamingTextByStreamId: {},
    streamingActivitiesByStreamId: {},
    replySessionsByPendingEventId: {},
    pendingEventIdByStreamId: {},
    rawHeadlessEventsByActorId: {},
    previewSessionsByActorId: {},
    latestActorPreviewByActorId: {},
    latestActorTextByActorId: {},
    latestActorActivitiesByActorId: {},
    chatWindow: null,
    hasMoreHistory: cached?.hasMoreHistory ?? true,
    hasLoadedTail: false,
    isLoadingHistory: false,
    isChatWindowLoading: false,
  };
}

export function ensureGroupChatBucket(chatByGroup: Record<string, GroupChatBucket>, groupId: string): Record<string, GroupChatBucket> {
  const gid = String(groupId || "").trim();
  if (!gid || chatByGroup[gid]) return chatByGroup;
  return {
    ...chatByGroup,
    [gid]: getGroupChatBucket(chatByGroup, gid),
  };
}

export function selectChatBucketState(
  state: Pick<GroupStateSnapshot, "chatByGroup">,
  groupId: string,
): GroupChatBucket {
  const gid = String(groupId || "").trim();
  if (!gid) return EMPTY_CHAT_BUCKET;
  return state.chatByGroup[gid] || EMPTY_CHAT_BUCKET;
}

const HEADLESS_PREVIEW_SESSION_LIMIT = 6;

function deriveHeadlessPreviewIndex(
  prevPreviewSessionsByActorId: Record<string, HeadlessPreviewSession[]>,
  streamingEvents: LedgerEvent[],
  streamingTextByStreamId: Record<string, string>,
  streamingActivitiesByStreamId: Record<string, StreamingActivity[]>,
  replySessionsByPendingEventId: Record<string, StreamingReplySession>,
): {
  previewSessionsByActorId: Record<string, HeadlessPreviewSession[]>;
  latestActorPreviewByActorId: Record<string, HeadlessPreviewSession>;
  latestActorTextByActorId: Record<string, string>;
  latestActorActivitiesByActorId: Record<string, StreamingActivity[]>;
} {
  const previewSessionsByActorId: Record<string, HeadlessPreviewSession[]> = {};
  const latestActorPreviewByActorId: Record<string, HeadlessPreviewSession> = {};
  const latestActorTextByActorId: Record<string, string> = {};
  const latestActorActivitiesByActorId: Record<string, StreamingActivity[]> = {};
  const fallbackLatestByActorId = new Map<string, {
    updatedAt: string;
    text: string;
    activities: StreamingActivity[];
  }>();

  const sessionEntriesByPendingEventId = new Map<string, Array<{
    actorId: string;
    streamId: string;
    streamPhase: string;
    updatedAt: string;
    text: string;
    activities: StreamingActivity[];
    completed: boolean;
    transient: boolean;
  }>>();
  const pendingEventIdsByActorId = new Map<string, Set<string>>();

  const noteActorSession = (actorId: string, pendingEventId: string) => {
    const existing = pendingEventIdsByActorId.get(actorId);
    if (existing) {
      existing.add(pendingEventId);
      return;
    }
    pendingEventIdsByActorId.set(actorId, new Set([pendingEventId]));
  };

  const findPreviousPreview = (actorId: string, pendingEventId: string): HeadlessPreviewSession | undefined => {
    const previousSessions = prevPreviewSessionsByActorId[actorId] || [];
    return previousSessions.find((session) => String(session.pendingEventId || "").trim() === pendingEventId);
  };

  const normalizePreviewBlockId = (streamId: string, streamPhase: string): string => {
    return `${String(streamId || "").trim()}::${String(streamPhase || "default").trim() || "default"}`;
  };

  const mergePreviewBlocks = (
    previousBlocks: HeadlessPreviewBlock[],
    nextBlocks: HeadlessPreviewBlock[],
  ): HeadlessPreviewBlock[] => {
    const merged = new Map<string, HeadlessPreviewBlock>();
    for (const block of previousBlocks) {
      if (!block || !String(block.text || "").trim()) continue;
      merged.set(String(block.id || "").trim(), block);
    }
    for (const block of nextBlocks) {
      if (!block || !String(block.text || "").trim()) continue;
      merged.set(String(block.id || "").trim(), block);
    }
    return Array.from(merged.values()).sort((left, right) => {
      const leftTs = String(left.updatedAt || "").trim();
      const rightTs = String(right.updatedAt || "").trim();
      if (leftTs && rightTs && leftTs !== rightTs) return leftTs.localeCompare(rightTs);
      return String(left.id || "").localeCompare(String(right.id || ""));
    });
  };

  const mergePreviewActivities = (
    previousActivities: StreamingActivity[],
    nextActivities: StreamingActivity[],
  ): StreamingActivity[] => {
    return dedupeStreamingActivities([...(previousActivities || []), ...(nextActivities || [])]).sort((left, right) => {
      const leftTs = String(left.ts || "").trim();
      const rightTs = String(right.ts || "").trim();
      if (leftTs && rightTs && leftTs !== rightTs) return leftTs.localeCompare(rightTs);
      return String(left.id || "").localeCompare(String(right.id || ""));
    });
  };

  for (const event of Array.isArray(streamingEvents) ? streamingEvents : []) {
    if (String(event.kind || "").trim() !== "chat.message") continue;
    const actorId = String(event.by || "").trim();
    if (!actorId || actorId === "user") continue;
    const data = event.data && typeof event.data === "object"
      ? event.data as {
        text?: unknown;
        stream_id?: unknown;
        activities?: unknown;
        pending_event_id?: unknown;
        stream_phase?: unknown;
        transient_stream?: unknown;
      }
      : undefined;
    const streamId = String(data?.stream_id || "").trim();
    const pendingEventId = String(data?.pending_event_id || "").trim();
    const liveText = streamId ? String(streamingTextByStreamId[streamId] || "").trim() : "";
    const eventText = String(data?.text || "").trim();
    const text = liveText || eventText;
    const liveActivities = streamId ? (streamingActivitiesByStreamId[streamId] || []) : [];
    const fallbackActivities = Array.isArray(data?.activities) ? data.activities.filter(Boolean) as StreamingActivity[] : [];
    const activities = liveActivities.length > 0 ? liveActivities : fallbackActivities;
    if (!pendingEventId) {
      const candidateUpdatedAt = String(event.ts || "").trim();
      const previousFallback = fallbackLatestByActorId.get(actorId);
      if (!previousFallback || candidateUpdatedAt >= previousFallback.updatedAt) {
        fallbackLatestByActorId.set(actorId, {
          updatedAt: candidateUpdatedAt,
          text,
          activities,
        });
      }
      continue;
    }

    noteActorSession(actorId, pendingEventId);

    const sessionEntries = sessionEntriesByPendingEventId.get(pendingEventId);
    const nextEntry = {
      actorId,
      streamId,
      streamPhase: String(data?.stream_phase || "").trim().toLowerCase(),
      updatedAt: String(event.ts || "").trim(),
      text,
      activities,
      completed: !event._streaming,
      transient: Boolean(data?.transient_stream),
    };
    if (sessionEntries) {
      sessionEntries.push(nextEntry);
    } else {
      sessionEntriesByPendingEventId.set(pendingEventId, [nextEntry]);
    }

  }

  for (const session of Object.values(replySessionsByPendingEventId || {})) {
    const actorId = String(session?.actorId || "").trim();
    const pendingEventId = String(session?.pendingEventId || "").trim();
    if (!actorId || !pendingEventId) continue;
    noteActorSession(actorId, pendingEventId);
  }

  for (const [actorId, pendingEventIds] of pendingEventIdsByActorId.entries()) {
    const actorSessions = Array.from(pendingEventIds)
      .map((pendingEventId) => {
        const session = replySessionsByPendingEventId[pendingEventId];
        const entries = sessionEntriesByPendingEventId.get(pendingEventId) || [];
        const previousPreview = findPreviousPreview(actorId, pendingEventId);
        const previousBlocks = previousPreview?.transcriptBlocks || [];
        const previousActivities = previousPreview?.activities || [];
        const nextBlocks = entries
          .filter((entry) => String(entry.text || "").trim())
          .map((entry) => ({
            id: normalizePreviewBlockId(entry.streamId || pendingEventId, entry.streamPhase),
            streamId: entry.streamId,
            streamPhase: entry.streamPhase,
            text: entry.text,
            updatedAt: entry.updatedAt,
            completed: entry.completed,
            transient: entry.transient,
          } satisfies HeadlessPreviewBlock));
        const transcriptBlocks = mergePreviewBlocks(previousBlocks, nextBlocks);
        const activities = mergePreviewActivities(previousActivities, entries.flatMap((entry) => entry.activities || []));
        const currentStreamId = String(session?.currentStreamId || "").trim() || String(entries[entries.length - 1]?.streamId || "").trim();
        const streamPhase = String(
          entries.find((entry) => String(entry.streamId || "").trim() === currentStreamId)?.streamPhase
          || entries[entries.length - 1]?.streamPhase
          || previousPreview?.streamPhase
          || ""
        ).trim().toLowerCase();
        const updatedAt = String(
          entries[entries.length - 1]?.updatedAt
          || (Number.isFinite(Number(session?.updatedAt)) ? new Date(Number(session?.updatedAt)).toISOString() : "")
          || previousPreview?.updatedAt
          || ""
        ).trim();
        const latestText = String(
          transcriptBlocks[transcriptBlocks.length - 1]?.text
          || previousPreview?.latestText
          || ""
        ).trim();
        const phase = String(
          session?.phase
          || (entries.some((entry) => !entry.completed) ? "streaming" : "completed")
          || previousPreview?.phase
          || ""
        ).trim().toLowerCase();
        if (!updatedAt && transcriptBlocks.length <= 0 && activities.length <= 0 && !latestText) {
          return null;
        }
        return {
          actorId,
          pendingEventId,
          currentStreamId,
          phase,
          streamPhase,
          updatedAt,
          latestText,
          transcriptBlocks,
          activities,
        } satisfies HeadlessPreviewSession;
      })
      .filter((session): session is HeadlessPreviewSession => !!session)
      .sort((left, right) => {
        const leftTs = String(left.updatedAt || "").trim();
        const rightTs = String(right.updatedAt || "").trim();
        if (leftTs && rightTs && leftTs !== rightTs) return leftTs.localeCompare(rightTs);
        if (leftTs && !rightTs) return 1;
        if (!leftTs && rightTs) return -1;
        return String(left.pendingEventId || "").localeCompare(String(right.pendingEventId || ""));
      })
      .slice(-HEADLESS_PREVIEW_SESSION_LIMIT);

    if (actorSessions.length <= 0) continue;

    previewSessionsByActorId[actorId] = actorSessions;
    const latestPreview = actorSessions[actorSessions.length - 1];
    latestActorPreviewByActorId[actorId] = latestPreview;
    if (latestPreview.latestText) {
      latestActorTextByActorId[actorId] = latestPreview.latestText;
    }
    if (latestPreview.activities.length > 0) {
      latestActorActivitiesByActorId[actorId] = latestPreview.activities;
    }
  }

  for (const [actorId, fallback] of fallbackLatestByActorId.entries()) {
    const latestPreview = latestActorPreviewByActorId[actorId];
    const latestUpdatedAt = String(latestPreview?.updatedAt || "").trim();
    if (!latestUpdatedAt || fallback.updatedAt >= latestUpdatedAt) {
      if (fallback.text) {
        latestActorTextByActorId[actorId] = fallback.text;
      }
      if (fallback.activities.length > 0) {
        latestActorActivitiesByActorId[actorId] = fallback.activities;
      }
    }
  }

  return {
    previewSessionsByActorId,
    latestActorPreviewByActorId,
    latestActorTextByActorId,
    latestActorActivitiesByActorId,
  };
}

export function buildChatBucketPatch(
  state: GroupStateSnapshot,
  groupId: string,
  patch: Partial<GroupChatBucket>,
): Partial<GroupStateSnapshot> | null {
  const gid = String(groupId || "").trim();
  if (!gid) return null;

  const prev = state.chatByGroup[gid] || EMPTY_CHAT_BUCKET;
  const nextEvents = patch.events !== undefined ? patch.events : prev.events;
  const nextStreamingEvents = patch.streamingEvents !== undefined ? patch.streamingEvents : prev.streamingEvents;
  const prevStreamingTextByStreamId = prev.streamingTextByStreamId || {};
  const prevStreamingActivitiesByStreamId = prev.streamingActivitiesByStreamId || {};
  const prevReplySessionsByPendingEventId = prev.replySessionsByPendingEventId || {};
  const prevPendingEventIdByStreamId = prev.pendingEventIdByStreamId || {};
  const prevRawHeadlessEventsByActorId = prev.rawHeadlessEventsByActorId || {};
  const prevPreviewSessionsByActorId = prev.previewSessionsByActorId || Object.fromEntries(
    Object.entries(prev.latestActorPreviewByActorId || {}).map(([actorId, preview]) => [actorId, [preview]])
  );
  const nextStreamingTextByStreamId = patch.streamingTextByStreamId !== undefined
    ? patch.streamingTextByStreamId
    : prevStreamingTextByStreamId;
  const nextStreamingActivitiesByStreamId = patch.streamingActivitiesByStreamId !== undefined
    ? patch.streamingActivitiesByStreamId
    : prevStreamingActivitiesByStreamId;
  const nextReplySessionsByPendingEventId = patch.replySessionsByPendingEventId !== undefined
    ? patch.replySessionsByPendingEventId
    : prevReplySessionsByPendingEventId;
  const nextPendingEventIdByStreamId = patch.pendingEventIdByStreamId !== undefined
    ? patch.pendingEventIdByStreamId
    : prevPendingEventIdByStreamId;
  const nextRawHeadlessEventsByActorId = patch.rawHeadlessEventsByActorId !== undefined
    ? patch.rawHeadlessEventsByActorId
    : prevRawHeadlessEventsByActorId;
  const previewIndex = deriveHeadlessPreviewIndex(
    prevPreviewSessionsByActorId,
    nextStreamingEvents,
    nextStreamingTextByStreamId,
    nextStreamingActivitiesByStreamId,
    nextReplySessionsByPendingEventId,
  );
  const nextHasMoreHistory = patch.hasMoreHistory !== undefined ? patch.hasMoreHistory : prev.hasMoreHistory;
  if (patch.events !== undefined || patch.hasMoreHistory !== undefined) {
    saveGroupView(gid, {
      events: nextEvents,
      hasMoreHistory: nextHasMoreHistory,
    });
  }
  return {
    chatByGroup: {
      ...state.chatByGroup,
      [gid]: {
        events: nextEvents,
        streamingEvents: nextStreamingEvents,
        streamingTextByStreamId: nextStreamingTextByStreamId,
        streamingActivitiesByStreamId: nextStreamingActivitiesByStreamId,
        replySessionsByPendingEventId: nextReplySessionsByPendingEventId,
        pendingEventIdByStreamId: nextPendingEventIdByStreamId,
        rawHeadlessEventsByActorId: nextRawHeadlessEventsByActorId,
        previewSessionsByActorId: previewIndex.previewSessionsByActorId,
        latestActorPreviewByActorId: previewIndex.latestActorPreviewByActorId,
        latestActorTextByActorId: previewIndex.latestActorTextByActorId,
        latestActorActivitiesByActorId: previewIndex.latestActorActivitiesByActorId,
        chatWindow: patch.chatWindow !== undefined ? patch.chatWindow : prev.chatWindow,
        hasMoreHistory: nextHasMoreHistory,
        hasLoadedTail: patch.hasLoadedTail !== undefined ? patch.hasLoadedTail : prev.hasLoadedTail,
        isLoadingHistory: patch.isLoadingHistory !== undefined ? patch.isLoadingHistory : prev.isLoadingHistory,
        isChatWindowLoading: patch.isChatWindowLoading !== undefined ? patch.isChatWindowLoading : prev.isChatWindowLoading,
      },
    },
  };
}

export function resolveChatGroupId(state: Pick<GroupStateSnapshot, "selectedGroupId">, groupId?: string): string {
  return String(groupId || state.selectedGroupId || "").trim();
}

export function mergeLedgerEventStatuses(events: LedgerEvent[], statuses: Record<string, LedgerEventStatusPayload>): LedgerEvent[] {
  if (!events.length || !Object.keys(statuses).length) return events;
  let changed = false;
  const next = events.map((event) => {
    const eventId = String(event.id || "").trim();
    if (!eventId) return event;
    const patch = statuses[eventId];
    if (!patch) return event;
    changed = true;
    return {
      ...event,
      _read_status: patch.read_status ?? event._read_status,
      _ack_status: patch.ack_status ?? event._ack_status,
      _obligation_status: patch.obligation_status ?? event._obligation_status,
    };
  });
  return changed ? next : events;
}

export function updateReadThroughIndex(messages: LedgerEvent[], endIndex: number, actorId: string) {
  const next = messages.slice();
  let changed = false;
  for (let i = 0; i <= endIndex; i += 1) {
    const message = next[i];
    if (!message || message.kind !== "chat.message") continue;
    const readStatus: Record<string, boolean> | null =
      message._read_status && typeof message._read_status === "object" ? { ...message._read_status } : null;
    const obligationStatus =
      message._obligation_status && typeof message._obligation_status === "object"
        ? { ...message._obligation_status }
        : null;
    if (!readStatus || !Object.prototype.hasOwnProperty.call(readStatus, actorId)) continue;
    if (readStatus[actorId] === true) continue;
    readStatus[actorId] = true;
    if (
      obligationStatus &&
      Object.prototype.hasOwnProperty.call(obligationStatus, actorId) &&
      typeof obligationStatus[actorId] === "object"
    ) {
      obligationStatus[actorId] = { ...obligationStatus[actorId], read: true };
      next[i] = { ...message, _read_status: readStatus, _obligation_status: obligationStatus };
    } else {
      next[i] = { ...message, _read_status: readStatus };
    }
    changed = true;
  }
  return { next, changed };
}

export function updateAckAtIndex(messages: LedgerEvent[], index: number, actorId: string) {
  const next = messages.slice();
  const message = next[index];
  if (!message || message.kind !== "chat.message") return { next, changed: false };

  const ackStatus: Record<string, boolean> | null =
    message._ack_status && typeof message._ack_status === "object" ? { ...message._ack_status } : null;
  const obligationStatus =
    message._obligation_status && typeof message._obligation_status === "object"
      ? { ...message._obligation_status }
      : null;
  if (!ackStatus || !Object.prototype.hasOwnProperty.call(ackStatus, actorId) || ackStatus[actorId] === true) {
    return { next, changed: false };
  }

  ackStatus[actorId] = true;
  if (
    obligationStatus &&
    Object.prototype.hasOwnProperty.call(obligationStatus, actorId) &&
    typeof obligationStatus[actorId] === "object"
  ) {
    obligationStatus[actorId] = { ...obligationStatus[actorId], acked: true };
    next[index] = { ...message, _ack_status: ackStatus, _obligation_status: obligationStatus };
  } else {
    next[index] = { ...message, _ack_status: ackStatus };
  }
  return { next, changed: true };
}

export function updateReplyAtIndex(messages: LedgerEvent[], index: number, actorId: string) {
  const next = messages.slice();
  const message = next[index];
  if (!message || message.kind !== "chat.message") return { next, changed: false };

  const ackStatus: Record<string, boolean> | null =
    message._ack_status && typeof message._ack_status === "object" ? { ...message._ack_status } : null;
  const obligationStatus =
    message._obligation_status && typeof message._obligation_status === "object"
      ? { ...message._obligation_status }
      : null;
  if (
    !obligationStatus ||
    !Object.prototype.hasOwnProperty.call(obligationStatus, actorId) ||
    typeof obligationStatus[actorId] !== "object"
  ) {
    return { next, changed: false };
  }

  const previous = obligationStatus[actorId];
  if (previous.replied && previous.acked) {
    return { next, changed: false };
  }

  obligationStatus[actorId] = { ...previous, replied: true, acked: true };
  if (ackStatus && Object.prototype.hasOwnProperty.call(ackStatus, actorId)) {
    ackStatus[actorId] = true;
    next[index] = { ...message, _ack_status: ackStatus, _obligation_status: obligationStatus };
  } else {
    next[index] = { ...message, _obligation_status: obligationStatus };
  }
  return { next, changed: true };
}

export function buildShellGroupDoc(groupId: string, groups: GroupMeta[], cached: GroupViewSnapshot | null): GroupDoc | null {
  const gid = String(groupId || "").trim();
  if (!gid) return null;
  const meta = groups.find((group) => String(group.group_id || "") === gid) || null;
  if (cached?.groupDoc && String(cached.groupDoc.group_id || "") === gid) {
    const cachedDoc = cloneGroupDoc(cached.groupDoc);
    if (!cachedDoc) return null;
    if (!meta) return cachedDoc;
    return {
      ...cachedDoc,
      group_id: gid,
      title: meta.title ?? cachedDoc.title,
      topic: meta.topic ?? cachedDoc.topic,
      running: typeof meta.running === "boolean" ? meta.running : cachedDoc.running,
      state: meta.state ?? cachedDoc.state,
      runtime_status: meta.runtime_status || cachedDoc.runtime_status,
    };
  }

  if (!meta) return null;
  return {
    group_id: gid,
    title: meta.title,
    topic: meta.topic,
    running: meta.running,
    state: meta.state,
    runtime_status: meta.runtime_status,
  };
}

export function deriveRuntimeStatusFromActors(actors: Actor[] | undefined, fallback?: GroupRuntimeStatus | null): GroupRuntimeStatus {
  const actorList = Array.isArray(actors) ? actors : [];
  const runningActors = actorList.filter((actor) => !!actor?.running);
  return {
    lifecycle_state: String(fallback?.lifecycle_state || "active"),
    runtime_running: runningActors.length > 0,
    running_actor_count: runningActors.length,
    has_running_foreman: runningActors.some((actor) => String(actor.role || "").trim().toLowerCase() === "foreman"),
  };
}

export function patchGroupRuntimeStatus(
  groups: GroupMeta[],
  groupId: string,
  runtimeStatus: GroupRuntimeStatus,
): GroupMeta[] {
  const gid = String(groupId || "").trim();
  if (!gid) return groups;
  let changed = false;
  const next = groups.map((group) => {
    if (String(group.group_id || "").trim() !== gid) return group;
    changed = true;
    return {
      ...group,
      running: !!runtimeStatus.runtime_running,
      state: String(runtimeStatus.lifecycle_state || group.state || "active") as GroupMeta["state"],
      runtime_status: runtimeStatus,
    };
  });
  return changed ? next : groups;
}

export function buildPrimedGroupState(groupId: string, groups: GroupMeta[]) {
  const gid = String(groupId || "").trim();
  if (!gid) {
    return {
      groupDoc: null,
      actors: [],
      groupContext: null,
      groupSettings: null,
      groupPresentation: null,
    };
  }

  const cached = getCachedGroupView(gid);
  return {
    groupDoc: buildShellGroupDoc(gid, groups, cached),
    actors: cached?.actors || [],
    groupContext: cached?.groupContext || null,
    groupSettings: cached?.groupSettings || null,
    groupPresentation: cached?.groupPresentation || null,
  };
}

export function filterUiEvents(events: LedgerEvent[] | undefined): LedgerEvent[] {
  return Array.isArray(events) ? events.filter((ev) => ev && !RUNTIME_ONLY_UI_EVENT_KINDS.has(String(ev.kind || ""))) : [];
}
