// UI 状态管理 - 管理 UI 相关状态
import { create } from "zustand";

interface UIState {
  // 状态
  activeTab: string;
  busy: string;
  errorMsg: string;
  isTransitioning: boolean;
  sidebarOpen: boolean;
  showScrollButton: boolean;
  chatUnreadCount: number;
  isSmallScreen: boolean;

  // Actions
  setActiveTab: (tab: string) => void;
  setBusy: (busy: string) => void;
  setError: (msg: string) => void;
  showError: (msg: string) => void;
  dismissError: () => void;
  setTransitioning: (v: boolean) => void;
  setSidebarOpen: (v: boolean) => void;
  setShowScrollButton: (v: boolean) => void;
  setChatUnreadCount: (v: number) => void;
  incrementChatUnread: () => void;
  setSmallScreen: (v: boolean) => void;
}

let errorTimeoutId: number | null = null;

export const useUIStore = create<UIState>((set) => ({
  // 初始状态
  activeTab: "chat",
  busy: "",
  errorMsg: "",
  isTransitioning: false,
  sidebarOpen: true,
  showScrollButton: false,
  chatUnreadCount: 0,
  isSmallScreen: false,

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

  setTransitioning: (v) => set({ isTransitioning: v }),
  setSidebarOpen: (v) => set({ sidebarOpen: v }),
  setShowScrollButton: (v) => set({ showScrollButton: v }),
  setChatUnreadCount: (v) => set({ chatUnreadCount: v }),
  incrementChatUnread: () => set((state) => ({ chatUnreadCount: state.chatUnreadCount + 1 })),
  setSmallScreen: (v) => set({ isSmallScreen: v }),
}));
