// UI state store (tabs, sidebar, toasts, etc.).
import { create } from "zustand";

interface UINotice {
  message: string;
  actionLabel?: string;
  actionId?: string;
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
  showScrollButton: boolean;
  chatUnreadCount: number;
  isSmallScreen: boolean;
  chatFilter: "all" | "to_user" | "attention" | "task";
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
  toggleSidebarCollapsed: () => void;
  setShowScrollButton: (v: boolean) => void;
  setChatUnreadCount: (v: number) => void;
  incrementChatUnread: () => void;
  setSmallScreen: (v: boolean) => void;
  setChatFilter: (v: "all" | "to_user" | "attention" | "task") => void;
  setWebReadOnly: (v: boolean) => void;
  setSSEStatus: (v: "connected" | "connecting" | "disconnected") => void;
}

let errorTimeoutId: number | null = null;
let noticeTimeoutId: number | null = null;

// localStorage key for sidebar collapsed state
const SIDEBAR_COLLAPSED_KEY = "cccc-sidebar-collapsed";

function loadSidebarCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
  } catch {
    return false;
  }
}

function saveSidebarCollapsed(collapsed: boolean): void {
  try {
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(collapsed));
  } catch {
    // Ignore storage errors
  }
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
  showScrollButton: false,
  chatUnreadCount: 0,
  isSmallScreen: false,
  chatFilter: "all",
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
    const persistent = Boolean(notice.actionId && notice.actionLabel);
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
  toggleSidebarCollapsed: () =>
    set((state) => {
      const next = !state.sidebarCollapsed;
      saveSidebarCollapsed(next);
      return { sidebarCollapsed: next };
    }),
  setShowScrollButton: (v) => set({ showScrollButton: v }),
  setChatUnreadCount: (v) => set({ chatUnreadCount: v }),
  incrementChatUnread: () => set((state) => ({ chatUnreadCount: state.chatUnreadCount + 1 })),
  setSmallScreen: (v) => set({ isSmallScreen: v }),
  setChatFilter: (v) => set({ chatFilter: v }),
  setWebReadOnly: (v) => set({ webReadOnly: v }),
  setSSEStatus: (v) => set({ sseStatus: v }),
}));
