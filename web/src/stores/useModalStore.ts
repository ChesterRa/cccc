// Modal state store.
import { create } from "zustand";
import type { Actor, LedgerEvent, PresentationMessageRef } from "../types";

interface RelaySource {
  groupId: string;
  event: LedgerEvent;
}

interface PresentationViewerState {
  groupId: string;
  slotId: string;
  surface?: "modal" | "split";
  focusRef?: PresentationMessageRef | null;
  focusEventId?: string | null;
}

interface PresentationPinState {
  groupId: string;
  slotId: string;
}

type PresentationAttentionState = Record<string, Record<string, boolean>>;

interface ModalState {
  // Modal visibility state
  modals: {
    context: boolean;
    settings: boolean;
    search: boolean;
    relay: boolean;
    addActor: boolean;
    createGroup: boolean;
    groupEdit: boolean;
    inbox: boolean;
    mobileMenu: boolean;
  };
  recipientsEventId: string | null;
  relayEventId: string | null;
  relaySource: RelaySource | null;
  contextTaskId: string | null;
  presentationViewer: PresentationViewerState | null;
  presentationPin: PresentationPinState | null;
  presentationAttention: PresentationAttentionState;
  editingActor: Actor | null;
  settingsTarget: { scope?: "group" | "global"; tab?: string; nonce: number } | null;

  // Actions
  openModal: (name: keyof ModalState["modals"]) => void;
  closeModal: (name: keyof ModalState["modals"]) => void;
  openSettingsTarget: (target: { scope?: "group" | "global"; tab?: string }) => void;
  clearSettingsTarget: () => void;
  setRecipientsModal: (eventId: string | null) => void;
  setRelayModal: (eventId: string | null, groupId?: string, event?: LedgerEvent | null) => void;
  openContextTask: (taskId: string) => void;
  clearContextTask: () => void;
  setPresentationViewer: (viewer: PresentationViewerState | null) => void;
  setPresentationPin: (pin: PresentationPinState | null) => void;
  markPresentationSlotAttention: (groupId: string, slotId: string) => void;
  clearPresentationSlotAttention: (groupId: string, slotId: string) => void;
  setEditingActor: (actor: Actor | null) => void;
}

export const useModalStore = create<ModalState>((set) => ({
  modals: {
    context: false,
    settings: false,
    search: false,
    relay: false,
    addActor: false,
    createGroup: false,
    groupEdit: false,
    inbox: false,
    mobileMenu: false,
  },
  recipientsEventId: null,
  relayEventId: null,
  relaySource: null,
  contextTaskId: null,
  presentationViewer: null,
  presentationPin: null,
  presentationAttention: {},
  editingActor: null,
  settingsTarget: null,

  openModal: (name) =>
    set((state) => ({
      modals: { ...state.modals, [name]: true },
    })),

  closeModal: (name) =>
    set((state) => ({
      modals: { ...state.modals, [name]: false },
      ...(name === "context" ? { contextTaskId: null } : {}),
      ...(name === "settings" ? { settingsTarget: null } : {}),
    })),

  openSettingsTarget: (target) =>
    set((state) => ({
      modals: { ...state.modals, settings: true },
      settingsTarget: {
        scope: target.scope === "global" ? "global" : target.scope === "group" ? "group" : undefined,
        tab: typeof target.tab === "string" ? target.tab : undefined,
        nonce: Date.now(),
      },
    })),
  clearSettingsTarget: () => set({ settingsTarget: null }),

  setRecipientsModal: (eventId) => set({ recipientsEventId: eventId }),
  setRelayModal: (eventId, groupId, event) =>
    set((state) => ({
      relayEventId: eventId,
      relaySource: eventId && groupId && event ? { groupId, event } : null,
      modals: { ...state.modals, relay: !!eventId },
    })),
  openContextTask: (taskId) =>
    set((state) => ({
      contextTaskId: String(taskId || "").trim() || null,
      modals: { ...state.modals, context: true },
    })),
  clearContextTask: () => set({ contextTaskId: null }),
  setPresentationViewer: (viewer) =>
    set((state) => {
      if (!viewer) {
        return { presentationViewer: null };
      }
      const groupId = String(viewer.groupId || "").trim();
      const slotId = String(viewer.slotId || "").trim();
      if (!groupId || !slotId) {
        return { presentationViewer: null };
      }
      const nextAttention = { ...state.presentationAttention };
      const groupAttention = { ...(nextAttention[groupId] || {}) };
      delete groupAttention[slotId];
      if (Object.keys(groupAttention).length > 0) {
        nextAttention[groupId] = groupAttention;
      } else {
        delete nextAttention[groupId];
      }
      return {
        presentationViewer: {
          groupId,
          slotId,
          surface: viewer.surface === "split" ? "split" : "modal",
          focusRef: viewer.focusRef || null,
          focusEventId: viewer.focusEventId ? String(viewer.focusEventId).trim() : null,
        },
        presentationAttention: nextAttention,
      };
    }),
  setPresentationPin: (pin) => set({ presentationPin: pin }),
  markPresentationSlotAttention: (groupId, slotId) =>
    set((state) => {
      const normalizedGroupId = String(groupId || "").trim();
      const normalizedSlotId = String(slotId || "").trim();
      if (!normalizedGroupId || !normalizedSlotId) return {};
      const viewer = state.presentationViewer;
      if (
        viewer &&
        viewer.groupId === normalizedGroupId &&
        viewer.slotId === normalizedSlotId
      ) {
        return {};
      }
      return {
        presentationAttention: {
          ...state.presentationAttention,
          [normalizedGroupId]: {
            ...(state.presentationAttention[normalizedGroupId] || {}),
            [normalizedSlotId]: true,
          },
        },
      };
    }),
  clearPresentationSlotAttention: (groupId, slotId) =>
    set((state) => {
      const normalizedGroupId = String(groupId || "").trim();
      const normalizedSlotId = String(slotId || "").trim();
      if (!normalizedGroupId || !normalizedSlotId) return {};
      const currentGroup = state.presentationAttention[normalizedGroupId];
      if (!currentGroup || !currentGroup[normalizedSlotId]) return {};
      const nextGroup = { ...currentGroup };
      delete nextGroup[normalizedSlotId];
      const nextAttention = { ...state.presentationAttention };
      if (Object.keys(nextGroup).length > 0) {
        nextAttention[normalizedGroupId] = nextGroup;
      } else {
        delete nextAttention[normalizedGroupId];
      }
      return { presentationAttention: nextAttention };
    }),
  setEditingActor: (actor) => set({ editingActor: actor }),
}));
