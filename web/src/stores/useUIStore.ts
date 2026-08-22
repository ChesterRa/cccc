// UI state store (tabs, sidebar, toasts, etc.).
import { create } from "zustand";
import {
  clampPresentationSplitWidth,
  PRESENTATION_SPLIT_DEFAULT_WIDTH,
} from "../utils/presentationSplitLayout";

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

export interface ChatSessionState {
  showScrollButton: boolean;
  chatUnreadCount: number;
  chatFilter: ChatFilter;
  scrollSnapshot: ChatScrollSnapshot | null;
  mobileSurface: "messages" | "presentation";
  presentationDockOpen: boolean;
  presentationDisplayMode: "modal" | "split";
}

const DEFAULT_CHAT_SESSION: ChatSessionState = {
  showScrollButton: false,
  chatUnreadCount: 0,
  chatFilter: "all",
  scrollSnapshot: null,
  mobileSurface: "messages",
  presentationDockOpen: false,
  presentationDisplayMode: "modal",
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
  presentationSplitWidth: number;
  chatSessions: Record<string, ChatSessionState>;
  webReadOnly: boolean;
  sseStatus: "connected" | "connecting" | "disconnected";

  // Actions
  setActiveTab: (tab: string) => void;
  setBusy: (busy: string) => void;
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
  setPresentationSplitWidth: (v: number) => void;
  setChatFilter: (groupId: string, v: ChatFilter) => void;
  setChatScrollSnapshot: (groupId: string, snap: ChatScrollSnapshot | null) => void;
  setChatMobileSurface: (groupId: string, v: "messages" | "presentation") => void;
  setChatPresentationDockOpen: (groupId: string, v: boolean) => void;
  setChatPresentationDisplayMode: (groupId: string, v: "modal" | "split") => void;
  setWebReadOnly: (v: boolean) => void;
  setSSEStatus: (v: "connected" | "connecting" | "disconnected") => void;
}

let errorTimeoutId: number | null = null;
let noticeTimeoutId: number | null = null;

// localStorage key for sidebar collapsed state
const SIDEBAR_COLLAPSED_KEY = "cccc-sidebar-collapsed";
const SIDEBAR_WIDTH_KEY = "cccc-sidebar-width";
const PRESENTATION_SPLIT_WIDTH_KEY = "cccc-presentation-split-width";
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

function loadPresentationSplitWidth(): number {
  try {
    return clampPresentationSplitWidth(Number(localStorage.getItem(PRESENTATION_SPLIT_WIDTH_KEY)));
  } catch (e) {
    console.warn("Failed to read presentation split width from localStorage:", e);
    return PRESENTATION_SPLIT_DEFAULT_WIDTH;
  }
}

function savePresentationSplitWidth(width: number): void {
  try {
    localStorage.setItem(PRESENTATION_SPLIT_WIDTH_KEY, String(clampPresentationSplitWidth(width)));
  } catch (e) {
    console.warn("Failed to persist presentation split width to localStorage:", e);
  }
}

function sanitizeChatSessions(value: unknown): Record<string, ChatSessionState> {
  if (!value || typeof value !== "object") return {};
  const input = value as Record<string, unknown>;
  const next: Record<string, ChatSessionState> = {};
  for (const [groupId, raw] of Object.entries(input)) {
    const gid = String(groupId || "").trim();
    if (!gid || !raw || typeof raw !== "object") continue;
    const session = raw as {
      chatFilter?: unknown;
      mobileSurface?: unknown;
      presentationDockOpen?: unknown;
      presentationDisplayMode?: unknown;
    };
    next[gid] = {
      ...DEFAULT_CHAT_SESSION,
      chatFilter:
        session.chatFilter === "user" ||
        session.chatFilter === "mail" ||
        session.chatFilter === "request_reply"
          ? session.chatFilter
          : "all",
      scrollSnapshot: null,
      mobileSurface: session.mobileSurface === "presentation" ? "presentation" : "messages",
      presentationDockOpen: Boolean(session.presentationDockOpen),
      presentationDisplayMode: session.presentationDisplayMode === "split" ? "split" : "modal",
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
          chatFilter: session.chatFilter,
          mobileSurface: session.mobileSurface,
          presentationDockOpen: session.presentationDockOpen,
          presentationDisplayMode: session.presentationDisplayMode,
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
  errorMsg: "",
  notice: null,
  isTransitioning: false,
  sidebarOpen: true,
  sidebarCollapsed: loadSidebarCollapsed(),
  sidebarWidth: loadSidebarWidth(),
  isSmallScreen: false,
  presentationSplitWidth: loadPresentationSplitWidth(),
  chatSessions: loadChatSessions(),
  webReadOnly: false,
  sseStatus: "disconnected" as const,

  // Actions
  setActiveTab: (tab) => set({ activeTab: tab }),
  setBusy: (busy) => set({ busy }),
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
  setPresentationSplitWidth: (v) => {
    const next = clampPresentationSplitWidth(v);
    savePresentationSplitWidth(next);
    set({ presentationSplitWidth: next });
  },
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
  setWebReadOnly: (v) => set({ webReadOnly: v }),
  setSSEStatus: (v) => set({ sseStatus: v }),
}));
