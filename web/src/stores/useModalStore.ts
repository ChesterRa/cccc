// Modal 状态管理
import { create } from "zustand";
import type { Actor } from "../types";

interface ModalState {
  // Modal 开关状态
  modals: {
    context: boolean;
    settings: boolean;
    search: boolean;
    addActor: boolean;
    createGroup: boolean;
    groupEdit: boolean;
    inbox: boolean;
    mobileMenu: boolean;
  };
  recipientsEventId: string | null;
  editingActor: Actor | null;

  // Actions
  openModal: (name: keyof ModalState["modals"]) => void;
  closeModal: (name: keyof ModalState["modals"]) => void;
  setRecipientsModal: (eventId: string | null) => void;
  setEditingActor: (actor: Actor | null) => void;
}

export const useModalStore = create<ModalState>((set) => ({
  modals: {
    context: false,
    settings: false,
    search: false,
    addActor: false,
    createGroup: false,
    groupEdit: false,
    inbox: false,
    mobileMenu: false,
  },
  recipientsEventId: null,
  editingActor: null,

  openModal: (name) =>
    set((state) => ({
      modals: { ...state.modals, [name]: true },
    })),

  closeModal: (name) =>
    set((state) => ({
      modals: { ...state.modals, [name]: false },
    })),

  setRecipientsModal: (eventId) => set({ recipientsEventId: eventId }),
  setEditingActor: (actor) => set({ editingActor: actor }),
}));
