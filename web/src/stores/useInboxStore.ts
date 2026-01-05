// Inbox 状态管理
import { create } from "zustand";
import type { LedgerEvent } from "../types";

interface InboxState {
  inboxActorId: string;
  inboxMessages: LedgerEvent[];

  // Actions
  setInboxActorId: (id: string) => void;
  setInboxMessages: (messages: LedgerEvent[]) => void;
  clearInbox: () => void;
}

export const useInboxStore = create<InboxState>((set) => ({
  inboxActorId: "",
  inboxMessages: [],

  setInboxActorId: (id) => set({ inboxActorId: id }),
  setInboxMessages: (messages) => set({ inboxMessages: messages }),
  clearInbox: () => set({ inboxActorId: "", inboxMessages: [] }),
}));
