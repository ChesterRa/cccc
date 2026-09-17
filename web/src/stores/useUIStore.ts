// UI state store (tabs, sidebar, toasts, etc.).
import { create } from "zustand";
import {
  clampComposerHeight,
  loadComposerHeight,
  saveComposerHeight,
} from "../utils/composerHeight";
import { clampSidePanelWidth, SIDE_PANEL_DEFAULT_WIDTH } from "../utils/sidePanelLayout";

export const SIDEBAR_COLLAPSED_WIDTH = 60;
export const SIDEBAR_DEFAULT_WIDTH = 248;
export const SIDEBAR_MIN_WIDTH = 248;
export const SIDEBAR_MAX_WIDTH = 360;
export const SIDEBAR_MAX_VIEWPORT_PERCENT = 34;

interface UINotice {
  message: string;
  actionLabel?: string;
  onAction?: () => void;
}

export type ChatFilter = "all" | "user" | "mail" | "request_reply";
export type ChatFollowMode = "follow" | "detached";
export const CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION = 1;

export interface ChatScrollSnapshot {
  coordinateVersion: typeof CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION;
  mode: ChatFollowMode;
  anchorId: string;
  offsetPx: number;
  scrollTop?: number;
  updatedAt: number;
}

export type GroupWorkView = "messages" | "terminals";
/** Which full-screen surface a phone shows for the selected group. */
export type MobileSurface = "messages" | "presentation" | "files";
const MOBILE_SURFACES: MobileSurface[] = ["messages", "presentation", "files"];

export interface ChatSessionState {
  workView: GroupWorkView;
  terminalPage: number;
  showScrollButton: boolean;
  chatUnreadCount: number;
  chatFilter: ChatFilter;
  scrollSnapshot: ChatScrollSnapshot | null;
  mobileSurface: MobileSurface;
  presentationDockOpen: boolean;
  presentationDisplayMode: "modal" | "split";
  filesPanelOpen: boolean;
  sidePanelWidth: number;
  presentationCompact: boolean;
}

const DEFAULT_CHAT_SESSION: ChatSessionState = {
  workView: "messages",
  terminalPage: 0,
  showScrollButton: false,
  chatUnreadCount: 0,
  chatFilter: "all",
  scrollSnapshot: null,
  mobileSurface: "messages",
  presentationDockOpen: false,
  presentationDisplayMode: "split",
  filesPanelOpen: false,
  sidePanelWidth: SIDE_PANEL_DEFAULT_WIDTH,
  presentationCompact: true,
};

export function getChatSession(
  groupId: string | null | undefined,
  sessions: Record<string, ChatSessionState>,
): ChatSessionState {
  const gid = String(groupId || "").trim();
  if (!gid) return DEFAULT_CHAT_SESSION;
  return sessions[gid] || DEFAULT_CHAT_SESSION;
}

interface UIState {
  // State
  activeTab: string;
  busy: string;
  errorMsg: string;
  notice: UINotice | null;
  isTransitioning: boolean;
  sidebarOpen: boolean;
  sidebarCollapsed: boolean; // Desktop sidebar collapsed state
  sidebarWidth: number;
  isSmallScreen: boolean;
  composerHeight: number | null;
  chatSessions: Record<string, ChatSessionState>;
  actorBusy: Record<string, number>;
  webReadOnly: boolean;
  workspaceFileViewerGroupId: string;
  sseStatus: "connected" | "connecting" | "disconnected";

  // Actions
  setActiveTab: (tab: string) => void;
  setBusy: (busy: string) => void;
  changeActorBusy: (groupId: string, actorId: string, delta: 1 | -1) => void;
  setError: (msg: string) => void;
  showError: (msg: string) => void;
  dismissError: () => void;
  showNotice: (notice: UINotice) => void;
  dismissNotice: () => void;
  setTransitioning: (v: boolean) => void;
  setSidebarOpen: (v: boolean) => void;
  setSidebarCollapsed: (v: boolean) => void;
  setSidebarWidth: (v: number) => void;
  toggleSidebarCollapsed: () => void;
  setShowScrollButton: (groupId: string, v: boolean) => void;
  setChatUnreadCount: (groupId: string, v: number) => void;
  incrementChatUnread: (groupId: string) => void;
  setSmallScreen: (v: boolean) => void;
  setChatSidePanelLayout: (groupId: string, layout: { width?: number; compact?: boolean }) => void;
  setComposerHeight: (v: number | null) => void;
  setChatFilter: (groupId: string, v: ChatFilter) => void;
  setChatScrollSnapshot: (groupId: string, snap: ChatScrollSnapshot | null) => void;
  setGroupWorkView: (groupId: string, view: GroupWorkView) => void;
  setGroupTerminalPage: (groupId: string, page: number) => void;
  setChatMobileSurface: (groupId: string, v: MobileSurface) => void;
  setChatPresentationDockOpen: (groupId: string, v: boolean) => void;
  setChatPresentationDisplayMode: (groupId: string, v: "modal" | "split") => void;
  setChatFilesPanelOpen: (groupId: string, v: boolean) => void;
  setWorkspaceFileViewerGroupId: (groupId: string) => void;
  setWebReadOnly: (v: boolean) => void;
  setSSEStatus: (v: "connected" | "connecting" | "disconnected") => void;
}

let errorTimeoutId: number | null = null;
let noticeTimeoutId: number | null = null;

// localStorage key for sidebar collapsed state
const SIDEBAR_COLLAPSED_KEY = "cccc-sidebar-collapsed";
const SIDEBAR_WIDTH_KEY = "cccc-sidebar-width";
const CHAT_SESSIONS_KEY = "cccc-chat-sessions";

export function clampSidebarWidth(value: number): number {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return SIDEBAR_DEFAULT_WIDTH;
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, Math.round(numeric)));
}

export function getSidebarWidthCssValue(value: number): string {
  const preferred = clampSidebarWidth(value);
  return `clamp(${SIDEBAR_MIN_WIDTH}px, ${preferred}px, min(${SIDEBAR_MAX_WIDTH}px, ${SIDEBAR_MAX_VIEWPORT_PERCENT}vw))`;
}

function loadSidebarCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
  } catch (e) {
    console.warn("Failed to read sidebar state from localStorage:", e);
    return false;
  }
}

function saveSidebarCollapsed(collapsed: boolean): void {
  try {
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(collapsed));
  } catch (e) {
    console.warn("Failed to persist sidebar state to localStorage:", e);
  }
}

function loadSidebarWidth(): number {
  try {
    return clampSidebarWidth(Number(localStorage.getItem(SIDEBAR_WIDTH_KEY)));
  } catch (e) {
    console.warn("Failed to read sidebar width from localStorage:", e);
    return SIDEBAR_DEFAULT_WIDTH;
  }
}

function saveSidebarWidth(width: number): void {
  try {
    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(clampSidebarWidth(width)));
  } catch (e) {
    console.warn("Failed to persist sidebar width to localStorage:", e);
  }
}

function sanitizeTerminalPage(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

export function groupMessagesVisible(
  groupId: string,
  state: Pick<
    UIState,
    "activeTab" | "chatSessions" | "isSmallScreen" | "workspaceFileViewerGroupId"
  >,
): boolean {
  const session = getChatSession(groupId, state.chatSessions);
  return (
    state.activeTab === "chat" &&
    state.workspaceFileViewerGroupId !== groupId &&
    session.workView !== "terminals" &&
    (!state.isSmallScreen || session.mobileSurface === "messages")
  );
}

function sanitizeChatSessions(value: unknown): Record<string, ChatSessionState> {
  if (!value || typeof value !== "object") return {};
  const input = value as Record<string, unknown>;
  const next: Record<string, ChatSessionState> = {};
  for (const [groupId, raw] of Object.entries(input)) {
    const gid = String(groupId || "").trim();
    if (!gid || !raw || typeof raw !== "object") continue;
    const session = raw as {
      workView?: unknown;
      terminalPage?: unknown;
      chatFilter?: unknown;
      mobileSurface?: unknown;
      presentationDockOpen?: unknown;
      presentationDisplayMode?: unknown;
      filesPanelOpen?: unknown;
      sidePanelWidth?: unknown;
      presentationCompact?: unknown;
    };
    next[gid] = {
      ...DEFAULT_CHAT_SESSION,
      workView: session.workView === "terminals" ? "terminals" : "messages",
      terminalPage: sanitizeTerminalPage(session.terminalPage),
      chatFilter:
        session.chatFilter === "user" ||
        session.chatFilter === "mail" ||
        session.chatFilter === "request_reply"
          ? session.chatFilter
          : "all",
      scrollSnapshot: null,
      mobileSurface: MOBILE_SURFACES.includes(session.mobileSurface as MobileSurface)
        ? (session.mobileSurface as MobileSurface)
        : "messages",
      presentationDockOpen: Boolean(session.presentationDockOpen),
      presentationDisplayMode: session.presentationDisplayMode === "modal" ? "modal" : "split",
      filesPanelOpen: Boolean(session.filesPanelOpen),
      sidePanelWidth: clampSidePanelWidth(Number(session.sidePanelWidth)),
      presentationCompact:
        typeof session.presentationCompact === "boolean" ? session.presentationCompact : true,
    };
  }
  return next;
}

function loadChatSessions(): Record<string, ChatSessionState> {
  try {
    const raw = localStorage.getItem(CHAT_SESSIONS_KEY);
    if (!raw) return {};
    return sanitizeChatSessions(JSON.parse(raw));
  } catch (e) {
    console.warn("Failed to read chat sessions from localStorage:", e);
    return {};
  }
}

function saveChatSessions(sessions: Record<string, ChatSessionState>): void {
  try {
    const persisted = Object.fromEntries(
      Object.entries(sessions).map(([groupId, session]) => [
        groupId,
        {
          workView: session.workView,
          terminalPage: session.terminalPage,
          chatFilter: session.chatFilter,
          mobileSurface: session.mobileSurface,
          presentationDockOpen: session.presentationDockOpen,
          presentationDisplayMode: session.presentationDisplayMode,
          filesPanelOpen: session.filesPanelOpen,
          sidePanelWidth: session.sidePanelWidth,
          presentationCompact: session.presentationCompact,
        },
      ]),
    );
    localStorage.setItem(CHAT_SESSIONS_KEY, JSON.stringify(persisted));
  } catch (e) {
    console.warn("Failed to persist chat sessions to localStorage:", e);
  }
}

function updateChatSession(
  sessions: Record<string, ChatSessionState>,
  groupId: string,
  patch: Partial<ChatSessionState>,
): Record<string, ChatSessionState> {
  const gid = String(groupId || "").trim();
  if (!gid) return sessions;
  const current = sessions[gid] || DEFAULT_CHAT_SESSION;
  const changed = (Object.keys(patch) as Array<keyof ChatSessionState>).some(
    (key) => !Object.is(current[key], patch[key]),
  );
  if (!changed) return sessions;
  return { ...sessions, [gid]: { ...current, ...patch } };
}

function updateChatSessionState(
  state: UIState,
  groupId: string,
  patch: Partial<ChatSessionState>,
): UIState | Pick<UIState, "chatSessions"> {
  const chatSessions = updateChatSession(state.chatSessions, groupId, patch);
  return chatSessions === state.chatSessions ? state : { chatSessions };
}

export const useUIStore = create<UIState>((set) => ({
  // Initial state
  activeTab: "chat",
  busy: "",
  actorBusy: {},
  errorMsg: "",
  notice: null,
  isTransitioning: false,
  sidebarOpen: true,
  sidebarCollapsed: loadSidebarCollapsed(),
  sidebarWidth: loadSidebarWidth(),
  isSmallScreen: false,
  composerHeight: loadComposerHeight(),
  chatSessions: loadChatSessions(),
  webReadOnly: false,
  workspaceFileViewerGroupId: "",
  sseStatus: "disconnected" as const,

  // Actions
  setActiveTab: (tab) => set({ activeTab: tab }),
  setBusy: (busy) => set({ busy }),
  changeActorBusy: (groupId, actorId, delta) =>
    set((state) => {
      const key = JSON.stringify([groupId, actorId]);
      const actorBusy = { ...state.actorBusy };
      const count = (actorBusy[key] || 0) + delta;
      if (count > 0) actorBusy[key] = count;
      else delete actorBusy[key];
      return { actorBusy };
    }),
  setError: (msg) => set({ errorMsg: msg }),

  showError: (msg) => {
    if (errorTimeoutId) window.clearTimeout(errorTimeoutId);
    set({ errorMsg: msg });
    errorTimeoutId = window.setTimeout(() => {
      set({ errorMsg: "" });
      errorTimeoutId = null;
    }, 8000);
  },

  dismissError: () => {
    if (errorTimeoutId) {
      window.clearTimeout(errorTimeoutId);
      errorTimeoutId = null;
    }
    set({ errorMsg: "" });
  },

  showNotice: (notice) => {
    if (noticeTimeoutId) {
      window.clearTimeout(noticeTimeoutId);
      noticeTimeoutId = null;
    }
    set({ notice });
    // Actionable notices remain until user dismisses/clicks action.
    const persistent = Boolean(notice.onAction && notice.actionLabel);
    if (!persistent) {
      noticeTimeoutId = window.setTimeout(() => {
        set({ notice: null });
        noticeTimeoutId = null;
      }, 3500);
    }
  },
  dismissNotice: () => {
    if (noticeTimeoutId) {
      window.clearTimeout(noticeTimeoutId);
      noticeTimeoutId = null;
    }
    set({ notice: null });
  },

  setTransitioning: (v) => set({ isTransitioning: v }),
  setSidebarOpen: (v) => set({ sidebarOpen: v }),
  setSidebarCollapsed: (v) => {
    saveSidebarCollapsed(v);
    set({ sidebarCollapsed: v });
  },
  setSidebarWidth: (v) => {
    const next = clampSidebarWidth(v);
    saveSidebarWidth(next);
    set({ sidebarWidth: next });
  },
  toggleSidebarCollapsed: () =>
    set((state) => {
      const next = !state.sidebarCollapsed;
      saveSidebarCollapsed(next);
      return { sidebarCollapsed: next };
    }),
  setShowScrollButton: (groupId, v) =>
    set((state) => updateChatSessionState(state, groupId, { showScrollButton: v })),
  setChatUnreadCount: (groupId, v) =>
    set((state) =>
      updateChatSessionState(state, groupId, { chatUnreadCount: Math.max(0, Number(v || 0)) }),
    ),
  incrementChatUnread: (groupId) =>
    set((state) => {
      const current = getChatSession(groupId, state.chatSessions);
      return {
        chatSessions: updateChatSession(state.chatSessions, groupId, {
          chatUnreadCount: current.chatUnreadCount + 1,
        }),
      };
    }),
  setSmallScreen: (v) => set({ isSmallScreen: v }),
  setComposerHeight: (v) => {
    const next = v === null ? null : clampComposerHeight(v);
    saveComposerHeight(next);
    set({ composerHeight: next });
  },
  setChatSidePanelLayout: (groupId, layout) =>
    set((state) => {
      const previous = getChatSession(groupId, state.chatSessions);
      const chatSessions = updateChatSession(state.chatSessions, groupId, {
        sidePanelWidth:
          layout.width === undefined ? previous.sidePanelWidth : clampSidePanelWidth(layout.width),
        presentationCompact: layout.compact ?? previous.presentationCompact,
      });
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setChatFilter: (groupId, v) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, { chatFilter: v });
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setChatScrollSnapshot: (groupId, snap) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, { scrollSnapshot: snap });
      if (chatSessions === state.chatSessions) return state;
      return { chatSessions };
    }),
  setGroupWorkView: (groupId, workView) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, { workView });
      if (chatSessions === state.chatSessions) return state;
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setGroupTerminalPage: (groupId, page) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, {
        terminalPage: sanitizeTerminalPage(page),
      });
      if (chatSessions === state.chatSessions) return state;
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setChatMobileSurface: (groupId, v) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, { mobileSurface: v });
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setChatPresentationDockOpen: (groupId, v) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, {
        presentationDockOpen: v,
      });
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setChatPresentationDisplayMode: (groupId, v) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, {
        presentationDisplayMode: v,
      });
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setChatFilesPanelOpen: (groupId, v) =>
    set((state) => {
      const chatSessions = updateChatSession(state.chatSessions, groupId, { filesPanelOpen: v });
      saveChatSessions(chatSessions);
      return { chatSessions };
    }),
  setWorkspaceFileViewerGroupId: (groupId) => set({ workspaceFileViewerGroupId: groupId }),
  setWebReadOnly: (v) => set({ webReadOnly: v }),
  setSSEStatus: (v) => set({ sseStatus: v }),
}));
