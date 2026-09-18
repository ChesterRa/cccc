// Inbox modal state store.
import { create } from "zustand";
import type { LedgerEvent } from "../types";

export interface InboxTarget {
  groupId: string;
  actorId: string;
}

interface InboxState {
  inboxTarget: InboxTarget | null;
  inboxMessages: LedgerEvent[];

  // Actions
  openInbox: (target: InboxTarget) => void;
  setInboxMessages: (target: InboxTarget, messages: LedgerEvent[]) => void;
  clearInbox: () => void;
}

export const useInboxStore = create<InboxState>((set) => ({
  inboxTarget: null,
  inboxMessages: [],

  openInbox: (target) => set({ inboxTarget: target, inboxMessages: [] }),
  // The target object identifies this opening, including reopenings of the same Actor.
  setInboxMessages: (target, messages) =>
    set((state) => (state.inboxTarget === target ? { inboxMessages: messages } : state)),
  clearInbox: () => set({ inboxTarget: null, inboxMessages: [] }),
}));
