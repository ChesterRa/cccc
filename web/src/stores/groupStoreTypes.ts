import type {
  Actor,
  GroupContext,
  GroupDoc,
  GroupMeta,
  GroupPresentation,
  GroupRuntimeStatus,
  GroupSettings,
  HeadlessStreamEvent,
  LedgerEvent,
  LedgerEventStatusPayload,
  RuntimeInfo,
  StreamingActivity,
} from "../types";
import type { ChatWindowState, GroupChatBucket } from "./groupStoreCore";

export interface GroupState {
  groups: GroupMeta[];
  groupOrder: string[];
  archivedGroupIds: string[];
  selectedGroupId: string;
  chatByGroup: Record<string, GroupChatBucket>;
  groupDoc: GroupDoc | null;
  events: LedgerEvent[];
  chatWindow: ChatWindowState | null;
  actors: Actor[];
  internalRuntimeActorsByGroup: Record<string, Actor[]>;
  groupContext: GroupContext | null;
  groupSettings: GroupSettings | null;
  groupPresentation: GroupPresentation | null;
  runtimes: RuntimeInfo[];
  selectedGroupActorsHydrating: boolean;
  hasMoreHistory: boolean;
  isLoadingHistory: boolean;
  isChatWindowLoading: boolean;

  setGroups: (groups: GroupMeta[]) => void;
  setGroupOrder: (order: string[]) => void;
  reorderGroupsInSection: (section: "working" | "archived", fromIndex: number, toIndex: number) => void;
  archiveGroup: (groupId: string) => void;
  restoreGroup: (groupId: string) => void;
  getOrderedGroups: () => GroupMeta[];
  setSelectedGroupId: (id: string) => void;
  setGroupDoc: (doc: GroupDoc | null) => void;
  setEvents: (events: LedgerEvent[], groupId?: string) => void;
  mergeEventStatuses: (statuses: Record<string, LedgerEventStatusPayload>, groupId?: string) => void;
  appendEvent: (event: LedgerEvent, groupId?: string) => void;
  appendHeadlessEvent: (event: HeadlessStreamEvent, groupId?: string) => void;
  upsertStreamingEvent: (event: LedgerEvent, groupId?: string) => void;
  upsertStreamingText: (streamId: string, text: string, groupId?: string) => void;
  upsertStreamingActivities: (streamId: string, activities: StreamingActivity[], groupId?: string) => void;
  upsertStreamingActivity: (
    actorId: string,
    match: { pendingEventId?: string; streamId?: string },
    activity: StreamingActivity,
    groupId?: string,
  ) => void;
  removeStreamingEvent: (streamId: string, groupId?: string) => void;
  removeStreamingEventsByPrefix: (streamIdPrefix: string, groupId?: string) => void;
  promoteStreamingEventsByPrefix: (streamIdPrefix: string, pendingEventId: string, groupId?: string) => void;
  promoteStreamingEventToStream: (
    actorId: string,
    pendingEventId: string,
    streamId: string,
    groupId?: string,
  ) => void;
  reconcileStreamingMessage: (args: {
    actorId: string;
    pendingEventId?: string;
    streamId: string;
    ts: string;
    fullText: string;
    eventText: string;
    activities: StreamingActivity[];
    completed: boolean;
    transientStream: boolean;
    phase?: string;
    groupId?: string;
  }) => void;
  completeStreamingEventsForActor: (actorId: string, groupId?: string) => void;
  clearStreamingEventsForActor: (actorId: string, groupId?: string) => void;
  clearEmptyStreamingEventsForActor: (actorId: string, groupId?: string) => void;
  clearTransientStreamingEventsForActor: (actorId: string, groupId?: string) => void;
  clearStreamingPlaceholder: (actorId: string, pendingEventId: string, groupId?: string) => void;
  prependEvents: (events: LedgerEvent[], groupId?: string) => void;
  setChatWindow: (w: GroupState["chatWindow"], groupId?: string) => void;
  setActors: (actors: Actor[]) => void;
  updateGroupRuntimeState: (groupId: string, patch: Partial<GroupRuntimeStatus>) => void;
  incrementActorUnread: (actorIds: string[]) => void;
  updateActorActivity: (updates: Array<{
    id: string;
    idle_seconds?: number | null;
    running: boolean;
    effective_working_state?: string;
    effective_working_reason?: string;
    effective_working_updated_at?: string | null;
    effective_active_task_id?: string | null;
  }>) => void;
  setGroupContext: (ctx: GroupContext | null) => void;
  setGroupSettings: (settings: GroupSettings | null) => void;
  setGroupPresentation: (presentation: GroupPresentation | null) => void;
  setRuntimes: (runtimes: RuntimeInfo[]) => void;
  updateReadStatus: (eventId: string, actorId: string, groupId?: string) => void;
  updateAckStatus: (eventId: string, actorId: string, groupId?: string) => void;
  updateReplyStatus: (eventId: string, actorId: string, groupId?: string) => void;
  setHasMoreHistory: (v: boolean, groupId?: string) => void;
  setIsLoadingHistory: (v: boolean, groupId?: string) => void;
  setIsChatWindowLoading: (v: boolean, groupId?: string) => void;

  refreshGroups: () => Promise<void>;
  refreshActors: (groupId?: string, opts?: { includeUnread?: boolean }) => Promise<void>;
  refreshInternalRuntimeActors: (groupId?: string) => Promise<void>;
  refreshSettings: (groupId?: string) => Promise<void>;
  refreshPresentation: (groupId?: string) => Promise<void>;
  loadGroup: (groupId: string) => Promise<void>;
  warmGroup: (groupId: string) => Promise<void>;
  loadMoreHistory: (groupId?: string) => Promise<void>;
  openChatWindow: (groupId: string, centerEventId: string) => Promise<void>;
  closeChatWindow: (groupId?: string) => void;
}

export type GroupStoreSet = (
  partial: GroupState | Partial<GroupState> | ((state: GroupState) => GroupState | Partial<GroupState>),
) => void;

export type GroupStoreGet = () => GroupState;

export type GroupStoreAsyncActions = Pick<
  GroupState,
  | "refreshGroups"
  | "refreshActors"
  | "refreshInternalRuntimeActors"
  | "refreshSettings"
  | "refreshPresentation"
  | "loadGroup"
  | "warmGroup"
  | "loadMoreHistory"
  | "openChatWindow"
  | "closeChatWindow"
>;
