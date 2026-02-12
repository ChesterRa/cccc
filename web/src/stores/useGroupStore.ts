// Group state store (groups, actors, events, context, settings).
import { create } from "zustand";
import type {
  GroupMeta,
  GroupDoc,
  LedgerEvent,
  Actor,
  RuntimeInfo,
  GroupContext,
  GroupSettings,
} from "../types";
import * as api from "../services/api";

interface GroupState {
  // Data
  groups: GroupMeta[];
  groupOrder: string[]; // Group IDs in user-defined order
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  events: LedgerEvent[];
  chatWindow: {
    groupId: string;
    centerEventId: string;
    centerIndex: number;
    events: LedgerEvent[];
    hasMoreBefore: boolean;
    hasMoreAfter: boolean;
  } | null;
  actors: Actor[];
  groupContext: GroupContext | null;
  groupSettings: GroupSettings | null;
  runtimes: RuntimeInfo[];
  hasMoreHistory: boolean;
  isLoadingHistory: boolean;
  isChatWindowLoading: boolean;

  // Actions
  setGroups: (groups: GroupMeta[]) => void;
  setGroupOrder: (order: string[]) => void;
  reorderGroups: (fromIndex: number, toIndex: number) => void;
  getOrderedGroups: () => GroupMeta[];
  setSelectedGroupId: (id: string) => void;
  setGroupDoc: (doc: GroupDoc | null) => void;
  setEvents: (events: LedgerEvent[]) => void;
  appendEvent: (event: LedgerEvent) => void;
  prependEvents: (events: LedgerEvent[]) => void;
  setChatWindow: (w: GroupState["chatWindow"]) => void;
  setActors: (actors: Actor[]) => void;
  setGroupContext: (ctx: GroupContext | null) => void;
  setGroupSettings: (settings: GroupSettings | null) => void;
  setRuntimes: (runtimes: RuntimeInfo[]) => void;
  updateReadStatus: (eventId: string, actorId: string) => void;
  updateAckStatus: (eventId: string, actorId: string) => void;
  updateReplyStatus: (eventId: string, actorId: string) => void;
  setHasMoreHistory: (v: boolean) => void;
  setIsLoadingHistory: (v: boolean) => void;
  setIsChatWindowLoading: (v: boolean) => void;

  // Async actions
  refreshGroups: () => Promise<void>;
  refreshActors: (groupId?: string) => Promise<void>;
  loadGroup: (groupId: string) => Promise<void>;
  loadMoreHistory: () => Promise<void>;
  openChatWindow: (groupId: string, centerEventId: string) => Promise<void>;
  closeChatWindow: () => void;
}

const MAX_UI_EVENTS = 800;

// localStorage key for group order
const GROUP_ORDER_KEY = "cccc-group-order";

function loadGroupOrder(): string[] {
  try {
    const stored = localStorage.getItem(GROUP_ORDER_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      if (Array.isArray(parsed)) return parsed;
    }
  } catch {
    // Ignore parse errors
  }
  return [];
}

function saveGroupOrder(order: string[]): void {
  try {
    localStorage.setItem(GROUP_ORDER_KEY, JSON.stringify(order));
  } catch {
    // Ignore storage errors
  }
}

// Merge stored order with current groups: preserve order for existing groups, append new ones at end
function mergeGroupOrder(storedOrder: string[], groups: GroupMeta[]): string[] {
  const currentIds = new Set(groups.map((g) => String(g.group_id || "")));
  // Keep only IDs that still exist in the current group list
  const validOrder = storedOrder.filter((id) => currentIds.has(id));
  // Find new groups not in stored order
  const orderedSet = new Set(validOrder);
  const newIds = groups
    .map((g) => String(g.group_id || ""))
    .filter((id) => id && !orderedSet.has(id));
  return [...validOrder, ...newIds];
}

// In-flight guards
let refreshGroupsInFlight = false;
let refreshGroupsQueued = false;
const refreshActorsInFlight = new Set<string>();
const refreshActorsQueued = new Set<string>();

export const useGroupStore = create<GroupState>((set, get) => ({
  // Initial state
  groups: [],
  groupOrder: loadGroupOrder(),
  selectedGroupId: "",
  groupDoc: null,
  events: [],
  chatWindow: null,
  actors: [],
  groupContext: null,
  groupSettings: null,
  runtimes: [],
  hasMoreHistory: true,
  isLoadingHistory: false,
  isChatWindowLoading: false,

  // Sync actions
  setGroups: (groups) => {
    const storedOrder = get().groupOrder;
    const mergedOrder = mergeGroupOrder(storedOrder, groups);
    saveGroupOrder(mergedOrder);
    set({ groups, groupOrder: mergedOrder });
  },
  setGroupOrder: (order) => {
    saveGroupOrder(order);
    set({ groupOrder: order });
  },
  reorderGroups: (fromIndex, toIndex) => {
    const order = get().groupOrder.slice();
    const [moved] = order.splice(fromIndex, 1);
    order.splice(toIndex, 0, moved);
    saveGroupOrder(order);
    set({ groupOrder: order });
  },
  getOrderedGroups: () => {
    const { groups, groupOrder } = get();
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
    return ordered;
  },
  setSelectedGroupId: (id) => set({ selectedGroupId: id }),
  setGroupDoc: (doc) => set({ groupDoc: doc }),
  setEvents: (events) => set({ events }),
  setChatWindow: (w) => set({ chatWindow: w }),
  appendEvent: (event) =>
    set((state) => {
      const next = state.events.concat([event]);
      return {
        events: next.length > MAX_UI_EVENTS ? next.slice(next.length - MAX_UI_EVENTS) : next,
      };
    }),
  prependEvents: (newEvents) =>
    set((state) => {
      // Deduplicate by id: filter out events that already exist
      const existingIds = new Set(state.events.map((e) => e.id).filter(Boolean));
      const uniqueNew = newEvents.filter((e) => e.id && !existingIds.has(e.id));
      const merged = [...uniqueNew, ...state.events];
      return {
        // Keep oldest when over limit (trim from end, preserving history)
        events: merged.length > MAX_UI_EVENTS ? merged.slice(0, MAX_UI_EVENTS) : merged,
      };
    }),
  setActors: (actors) => set({ actors }),
  setGroupContext: (ctx) => set({ groupContext: ctx }),
  setGroupSettings: (settings) => set({ groupSettings: settings }),
  setRuntimes: (runtimes) => set({ runtimes }),

  updateReadStatus: (eventId, actorId) =>
    set((state) => {
      const idx = state.events.findIndex(
        (x) => x.kind === "chat.message" && String(x.id || "") === eventId
      );
      if (idx < 0 && !state.chatWindow) return state;

      const next = state.events.slice();
      let didChange = false;

      if (idx >= 0) {
        for (let i = 0; i <= idx; i++) {
          const m = next[i];
          if (!m || m.kind !== "chat.message") continue;
          const rs: Record<string, boolean> | null =
            m._read_status && typeof m._read_status === "object" ? { ...m._read_status } : null;
          const os =
            m._obligation_status && typeof m._obligation_status === "object"
              ? { ...m._obligation_status }
              : null;
          // Only mark for actual recipients; never add keys for non-recipients.
          if (!rs || !Object.prototype.hasOwnProperty.call(rs, actorId)) continue;
          if (rs[actorId] === true) continue;
          rs[actorId] = true;
          if (os && Object.prototype.hasOwnProperty.call(os, actorId) && typeof os[actorId] === "object") {
            os[actorId] = { ...os[actorId], read: true };
            next[i] = { ...m, _read_status: rs, _obligation_status: os };
          } else {
            next[i] = { ...m, _read_status: rs };
          }
          didChange = true;
        }
      }

      let nextWindow = state.chatWindow;
      if (state.chatWindow && String(state.chatWindow.groupId || "") === String(state.selectedGroupId || "")) {
        const wNext = state.chatWindow.events.slice();
        const wIdx = wNext.findIndex((x) => x.kind === "chat.message" && String(x.id || "") === eventId);
        if (wIdx >= 0) {
          for (let i = 0; i <= wIdx; i++) {
            const m = wNext[i];
            if (!m || m.kind !== "chat.message") continue;
            const rs: Record<string, boolean> | null =
              m._read_status && typeof m._read_status === "object" ? { ...m._read_status } : null;
            const os =
              m._obligation_status && typeof m._obligation_status === "object"
                ? { ...m._obligation_status }
                : null;
            if (!rs || !Object.prototype.hasOwnProperty.call(rs, actorId)) continue;
            if (rs[actorId] === true) continue;
            rs[actorId] = true;
            if (os && Object.prototype.hasOwnProperty.call(os, actorId) && typeof os[actorId] === "object") {
              os[actorId] = { ...os[actorId], read: true };
              wNext[i] = { ...m, _read_status: rs, _obligation_status: os };
            } else {
              wNext[i] = { ...m, _read_status: rs };
            }
            didChange = true;
          }
          nextWindow = { ...state.chatWindow, events: wNext };
        }
      }

      if (!didChange) return state;
      return { events: next, chatWindow: nextWindow };
    }),

  updateAckStatus: (eventId, actorId) =>
    set((state) => {
      const idx = state.events.findIndex(
        (x) => x.kind === "chat.message" && String(x.id || "") === eventId
      );
      if (idx < 0 && !state.chatWindow) return state;

      const next = state.events.slice();
      const msg = idx >= 0 ? next[idx] : null;

      let didChange = false;

      if (msg && msg.kind === "chat.message") {
        const as: Record<string, boolean> | null =
          msg._ack_status && typeof msg._ack_status === "object" ? { ...msg._ack_status } : null;
        const os =
          msg._obligation_status && typeof msg._obligation_status === "object"
            ? { ...msg._obligation_status }
            : null;
        if (as && Object.prototype.hasOwnProperty.call(as, actorId) && as[actorId] !== true) {
          as[actorId] = true;
          if (os && Object.prototype.hasOwnProperty.call(os, actorId) && typeof os[actorId] === "object") {
            os[actorId] = { ...os[actorId], acked: true };
            next[idx] = { ...msg, _ack_status: as, _obligation_status: os };
          } else {
            next[idx] = { ...msg, _ack_status: as };
          }
          didChange = true;
        }
      }

      let nextWindow = state.chatWindow;
      if (state.chatWindow && String(state.chatWindow.groupId || "") === String(state.selectedGroupId || "")) {
        const wNext = state.chatWindow.events.slice();
        const wIdx = wNext.findIndex((x) => x.kind === "chat.message" && String(x.id || "") === eventId);
        if (wIdx >= 0) {
          const wMsg = wNext[wIdx];
          const as: Record<string, boolean> | null =
            wMsg && wMsg.kind === "chat.message" && wMsg._ack_status && typeof wMsg._ack_status === "object"
              ? { ...wMsg._ack_status }
              : null;
          const os =
            wMsg && wMsg.kind === "chat.message" && wMsg._obligation_status && typeof wMsg._obligation_status === "object"
              ? { ...wMsg._obligation_status }
              : null;
          if (as && Object.prototype.hasOwnProperty.call(as, actorId) && as[actorId] !== true) {
            as[actorId] = true;
            if (os && Object.prototype.hasOwnProperty.call(os, actorId) && typeof os[actorId] === "object") {
              os[actorId] = { ...os[actorId], acked: true };
              wNext[wIdx] = { ...wMsg, _ack_status: as, _obligation_status: os };
            } else {
              wNext[wIdx] = { ...wMsg, _ack_status: as };
            }
            nextWindow = { ...state.chatWindow, events: wNext };
            didChange = true;
          }
        }
      }

      if (!didChange) return state;
      return { events: next, chatWindow: nextWindow };
    }),

  updateReplyStatus: (eventId, actorId) =>
    set((state) => {
      const idx = state.events.findIndex(
        (x) => x.kind === "chat.message" && String(x.id || "") === eventId
      );
      if (idx < 0 && !state.chatWindow) return state;

      const next = state.events.slice();
      const msg = idx >= 0 ? next[idx] : null;

      let didChange = false;

      if (msg && msg.kind === "chat.message") {
        const as: Record<string, boolean> | null =
          msg._ack_status && typeof msg._ack_status === "object" ? { ...msg._ack_status } : null;
        const os =
          msg._obligation_status && typeof msg._obligation_status === "object"
            ? { ...msg._obligation_status }
            : null;
        if (os && Object.prototype.hasOwnProperty.call(os, actorId) && typeof os[actorId] === "object") {
          const prev = os[actorId];
          const changed = !prev.replied || !prev.acked;
          if (changed) {
            os[actorId] = { ...prev, replied: true, acked: true };
            if (as && Object.prototype.hasOwnProperty.call(as, actorId)) {
              as[actorId] = true;
              next[idx] = { ...msg, _ack_status: as, _obligation_status: os };
            } else {
              next[idx] = { ...msg, _obligation_status: os };
            }
            didChange = true;
          }
        }
      }

      let nextWindow = state.chatWindow;
      if (state.chatWindow && String(state.chatWindow.groupId || "") === String(state.selectedGroupId || "")) {
        const wNext = state.chatWindow.events.slice();
        const wIdx = wNext.findIndex((x) => x.kind === "chat.message" && String(x.id || "") === eventId);
        if (wIdx >= 0) {
          const wMsg = wNext[wIdx];
          const as: Record<string, boolean> | null =
            wMsg && wMsg.kind === "chat.message" && wMsg._ack_status && typeof wMsg._ack_status === "object"
              ? { ...wMsg._ack_status }
              : null;
          const os =
            wMsg && wMsg.kind === "chat.message" && wMsg._obligation_status && typeof wMsg._obligation_status === "object"
              ? { ...wMsg._obligation_status }
              : null;
          if (os && Object.prototype.hasOwnProperty.call(os, actorId) && typeof os[actorId] === "object") {
            const prev = os[actorId];
            const changed = !prev.replied || !prev.acked;
            if (changed) {
              os[actorId] = { ...prev, replied: true, acked: true };
              if (as && Object.prototype.hasOwnProperty.call(as, actorId)) {
                as[actorId] = true;
                wNext[wIdx] = { ...wMsg, _ack_status: as, _obligation_status: os };
              } else {
                wNext[wIdx] = { ...wMsg, _obligation_status: os };
              }
              nextWindow = { ...state.chatWindow, events: wNext };
              didChange = true;
            }
          }
        }
      }

      if (!didChange) return state;
      return { events: next, chatWindow: nextWindow };
    }),
  setHasMoreHistory: (v) => set({ hasMoreHistory: v }),
  setIsLoadingHistory: (v) => set({ isLoadingHistory: v }),
  setIsChatWindowLoading: (v) => set({ isChatWindowLoading: v }),

  // Async actions
  refreshGroups: async () => {
    if (refreshGroupsInFlight) {
      refreshGroupsQueued = true;
      return;
    }
    refreshGroupsInFlight = true;
    try {
      const resp = await api.fetchGroups();
      if (resp.ok) {
        const next = resp.result.groups || [];
        // Use setGroups to ensure groupOrder is updated
        get().setGroups(next);

        const cur = get().selectedGroupId;
        const curExists = !!cur && next.some((g) => String(g.group_id || "") === cur);
        if (!curExists) {
          if (next.length > 0) {
            // Selected group disappeared (or none selected): switch to first group
            // and clear stale per-group caches so UI does not render old data while switching.
            set({
              selectedGroupId: String(next[0].group_id || ""),
              groupDoc: null,
              events: [],
              actors: [],
              groupContext: null,
              groupSettings: null,
              chatWindow: null,
              hasMoreHistory: true,
            });
          } else {
            // No groups remain: clear selection + per-group caches.
            set({
              selectedGroupId: "",
              groupDoc: null,
              events: [],
              actors: [],
              groupContext: null,
              groupSettings: null,
              chatWindow: null,
              hasMoreHistory: false,
            });
          }
          return;
        }

        // Keep groupDoc's basic fields in sync with the group list. This matters when
        // group state/title/topic is changed externally (CLI/MCP/etc.), since groupDoc
        // is otherwise only updated by web actions or explicit loadGroup().
        const selectedId = get().selectedGroupId;
        const doc = get().groupDoc;
        if (doc && selectedId && String(doc.group_id || "") === String(selectedId || "")) {
          const meta = next.find((g) => String(g.group_id || "") === String(selectedId || "")) || null;
          if (meta) {
            const patch: Partial<GroupDoc> = {};
            if (typeof meta.state === "string" && meta.state !== doc.state) patch.state = meta.state;
            if (typeof meta.title === "string" && meta.title !== doc.title) patch.title = meta.title;
            if (typeof meta.topic === "string" && meta.topic !== doc.topic) patch.topic = meta.topic;
            if (Object.keys(patch).length > 0) {
              set({ groupDoc: { ...doc, ...patch } });
            }
          }
        }
      }
    } catch {
      // Ignore transient failures
    } finally {
      refreshGroupsInFlight = false;
      if (refreshGroupsQueued) {
        refreshGroupsQueued = false;
        void get().refreshGroups();
      }
    }
  },

  refreshActors: async (groupId?: string) => {
    const gid = String(groupId || get().selectedGroupId || "").trim();
    if (!gid) return;
    if (refreshActorsInFlight.has(gid)) {
      refreshActorsQueued.add(gid);
      return;
    }
    refreshActorsInFlight.add(gid);
    try {
      const resp = await api.fetchActors(gid);
      if (resp.ok && get().selectedGroupId === gid) {
        set({ actors: resp.result.actors || [] });
      }
    } catch {
      // Ignore transient failures
    } finally {
      refreshActorsInFlight.delete(gid);
      if (refreshActorsQueued.has(gid)) {
        refreshActorsQueued.delete(gid);
        void get().refreshActors(gid);
      }
    }
  },

  loadGroup: async (groupId: string) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;

    // First check if the group exists (goes through daemon, won't 404)
    const show = await api.fetchGroup(gid);

    // Guard against out-of-order resolves when the user switches groups quickly.
    if (get().selectedGroupId !== gid) return;

    // If group truly does not exist, clear state. For transient failures
    // (network/500), keep current UI state to avoid false "data vanished" flashes.
    if (!show.ok) {
      const code = String(show.error?.code || "").trim();
      if (code === "group_not_found") {
        set({
          groupDoc: null,
          events: [],
          actors: [],
          groupContext: null,
          groupSettings: null,
          hasMoreHistory: false,
        });
      }
      return;
    }

    // Fast initial load: skip unread counts for snappy group switching
    const [tail, a, ctx, settings] = await Promise.all([
      api.fetchLedgerTail(gid),
      api.fetchActors(gid, false),
      api.fetchContext(gid),
      api.fetchSettings(gid),
    ]);

    // Guard against out-of-order resolves when the user switches groups quickly.
    if (get().selectedGroupId !== gid) return;

    set({
      groupDoc: show.result.group,
      events: tail.ok
        ? (tail.result.events || []).filter((ev) => ev && ev.kind !== "context.sync")
        : [],
      actors: a.ok ? a.result.actors || [] : [],
      groupContext: ctx.ok ? (ctx.result as GroupContext) : null,
      groupSettings: settings.ok && settings.result.settings ? settings.result.settings : null,
      hasMoreHistory: tail.ok ? !!tail.result.has_more : true,
    });

    // Deferred: load unread counts in background (doesn't block UI)
    setTimeout(() => {
      if (get().selectedGroupId === gid) {
        get().refreshActors(gid);
      }
    }, 300);
  },

  loadMoreHistory: async () => {
    const { selectedGroupId, events, isLoadingHistory, hasMoreHistory } = get();
    if (!selectedGroupId) return;
    if (isLoadingHistory || !hasMoreHistory) return;

    // Find first chat message to use as cursor
    const chatMessages = events.filter((ev) => ev.kind === "chat.message");
    const firstEvent = chatMessages[0];
    if (!firstEvent?.id) return;

    set({ isLoadingHistory: true });
    try {
      const resp = await api.fetchOlderMessages(selectedGroupId, String(firstEvent.id), 50);
      // Guard against group switch during loading
      if (get().selectedGroupId !== selectedGroupId) return;

      if (resp.ok) {
        // Filter to only chat messages (same as initial load)
        const olderChatEvents = (resp.result.events || []).filter(
          (ev) => ev && ev.kind === "chat.message"
        );
        // Deduplicate and prepend
        const existingIds = new Set(get().events.map((e) => e.id).filter(Boolean));
        const uniqueNew = olderChatEvents.filter((e) => e.id && !existingIds.has(e.id));
        const merged = [...uniqueNew, ...get().events];
        set({
          events: merged.length > MAX_UI_EVENTS ? merged.slice(0, MAX_UI_EVENTS) : merged,
          hasMoreHistory: resp.result.has_more,
        });
      }
    } finally {
      set({ isLoadingHistory: false });
    }
  },

  openChatWindow: async (groupId: string, centerEventId: string) => {
    const gid = String(groupId || "").trim();
    const eid = String(centerEventId || "").trim();
    if (!gid || !eid) return;

    set({
      isChatWindowLoading: true,
      chatWindow: {
        groupId: gid,
        centerEventId: eid,
        centerIndex: 0,
        events: [],
        hasMoreBefore: false,
        hasMoreAfter: false,
      },
    });
    try {
      const resp = await api.fetchMessageWindow(gid, eid, { before: 30, after: 30 });
      if (!resp.ok) {
        set({ chatWindow: null });
        return;
      }
      // Guard against group switch during load.
      if (get().selectedGroupId !== gid) return;

      const events = (resp.result.events || []).filter((ev) => ev && ev.kind === "chat.message");
      set({
        chatWindow: {
          groupId: gid,
          centerEventId: resp.result.center_id,
          centerIndex: resp.result.center_index,
          events,
          hasMoreBefore: !!resp.result.has_more_before,
          hasMoreAfter: !!resp.result.has_more_after,
        },
      });
    } finally {
      set({ isChatWindowLoading: false });
    }
  },

  closeChatWindow: () => set({ chatWindow: null, isChatWindowLoading: false }),
}));
