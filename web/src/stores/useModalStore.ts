// Modal state store.
import { create } from "zustand";
import type { Actor, LedgerEvent, PresentationMessageRef, SupportedRuntime } from "../types";
import { formatRuntimeCommand } from "../components/modals/runtimeProfileControlsModel";
import { formatCapabilityIdInput } from "../utils/capabilityAutoload";
import { useFormStore } from "./useFormStore";

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
  /** Keep the resource tab's reference identity when moving between a panel and a window. */
  refKey?: string;
  /** A window opened from a resource tab observes its existing shared session. */
  reuseExistingSession?: boolean;
}

interface PresentationPinState {
  groupId: string;
  slotId: string;
  /** Preselects a workspace file, so pinning from the file tree skips the picker. */
  workspacePath?: string;
}

type PresentationAttentionState = Record<string, Record<string, boolean>>;
type VisiblePresentationResource = { groupId: string; slotId: string };

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
  groupConnectionsId: string | null;
  /** The Group the capability center was opened for; "" opens it without one, null keeps it closed. */
  capabilityCenterGroupId: string | null;
  presentationViewer: PresentationViewerState | null;
  presentationPin: PresentationPinState | null;
  presentationAttention: PresentationAttentionState;
  /** Mounted resource viewers register only while the selected slot is actually on screen. */
  visiblePresentationResources: Map<symbol, VisiblePresentationResource>;
  editingActor: Actor | null;
  editingActorSection: "chatgpt" | null;
  settingsTarget: {
    scope?: "group" | "global";
    tab?: string;
    webModelProvider?: "chatgpt_web" | "grok_web" | "dots";
    webModelSection?: "login" | "connector";
    voiceSection?: "secretary" | "realtime";
    voiceAudio?: boolean;
    voiceAnalyst?: boolean;
    nonce: number;
  } | null;

  // Actions
  openModal: (name: keyof ModalState["modals"]) => void;
  closeModal: (name: keyof ModalState["modals"]) => void;
  openSettingsTarget: (target: {
    scope?: "group" | "global";
    tab?: string;
    webModelProvider?: "chatgpt_web" | "grok_web" | "dots";
    webModelSection?: "login" | "connector";
    voiceSection?: "secretary" | "realtime";
    voiceAudio?: boolean;
    voiceAnalyst?: boolean;
  }) => void;
  clearSettingsTarget: () => void;
  openCodexVoiceSettings: () => void;
  setRecipientsModal: (eventId: string | null) => void;
  setRelayModal: (eventId: string | null, groupId?: string, event?: LedgerEvent | null) => void;
  openContextTask: (taskId: string) => void;
  clearContextTask: () => void;
  setGroupConnections: (groupId: string | null) => void;
  setCapabilityCenter: (groupId: string | null) => void;
  setPresentationViewer: (viewer: PresentationViewerState | null) => void;
  setPresentationPin: (pin: PresentationPinState | null) => void;
  setVisiblePresentationResource: (
    owner: symbol,
    viewer: VisiblePresentationResource | null,
  ) => void;
  markPresentationSlotAttention: (groupId: string, slotId: string) => void;
  clearPresentationSlotAttention: (groupId: string, slotId: string) => void;
  openActorEditor: (actor: Actor, section?: "chatgpt") => void;
  // Update the open snapshot (e.g. avatar) or close without resetting its draft.
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
  groupConnectionsId: null,
  capabilityCenterGroupId: null,
  presentationViewer: null,
  presentationPin: null,
  presentationAttention: {},
  visiblePresentationResources: new Map(),
  editingActor: null,
  editingActorSection: null,
  settingsTarget: null,

  openModal: (name) => set((state) => ({ modals: { ...state.modals, [name]: true } })),

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
        scope:
          target.scope === "global" ? "global" : target.scope === "group" ? "group" : undefined,
        tab: typeof target.tab === "string" ? target.tab : undefined,
        webModelProvider: target.webModelProvider,
        webModelSection: target.webModelSection,
        voiceSection: target.voiceSection,
        voiceAudio: target.voiceAudio,
        voiceAnalyst: target.voiceAnalyst,
        nonce: Date.now(),
      },
    })),
  clearSettingsTarget: () => set({ settingsTarget: null }),
  openCodexVoiceSettings: () =>
    set((state) => ({
      modals: { ...state.modals, settings: true },
      settingsTarget: {
        scope: "global",
        tab: "voice",
        voiceSection: "realtime",
        nonce: Date.now(),
      },
    })),
  setGroupConnections: (groupId) => set({ groupConnectionsId: groupId }),
  setCapabilityCenter: (groupId) =>
    set({ capabilityCenterGroupId: groupId === null ? null : String(groupId || "").trim() }),

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
          refKey: viewer.refKey,
          reuseExistingSession: viewer.reuseExistingSession === true,
        },
        presentationAttention: nextAttention,
      };
    }),
  setPresentationPin: (pin) => set({ presentationPin: pin }),
  setVisiblePresentationResource: (owner, viewer) =>
    set((state) => {
      const groupId = String(viewer?.groupId || "").trim();
      const slotId = String(viewer?.slotId || "").trim();
      const existing = state.visiblePresentationResources.get(owner);
      if (existing?.groupId === groupId && existing.slotId === slotId) return {};
      if (!existing && (!groupId || !slotId)) return {};
      const visible = new Map(state.visiblePresentationResources);
      if (groupId && slotId) visible.set(owner, { groupId, slotId });
      else visible.delete(owner);
      if (!groupId || !slotId || !state.presentationAttention[groupId]?.[slotId]) {
        return { visiblePresentationResources: visible };
      }
      const nextAttention = { ...state.presentationAttention };
      const groupAttention = { ...(nextAttention[groupId] || {}) };
      delete groupAttention[slotId];
      if (Object.keys(groupAttention).length) nextAttention[groupId] = groupAttention;
      else delete nextAttention[groupId];
      return { visiblePresentationResources: visible, presentationAttention: nextAttention };
    }),
  markPresentationSlotAttention: (groupId, slotId) =>
    set((state) => {
      const normalizedGroupId = String(groupId || "").trim();
      const normalizedSlotId = String(slotId || "").trim();
      if (!normalizedGroupId || !normalizedSlotId) return {};
      const viewer = state.presentationViewer;
      if (viewer && viewer.groupId === normalizedGroupId && viewer.slotId === normalizedSlotId) {
        return {};
      }
      if (
        [...state.visiblePresentationResources.values()].some(
          (resource) =>
            resource.groupId === normalizedGroupId && resource.slotId === normalizedSlotId,
        )
      )
        return {};
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
  openActorEditor: (actor, section) => {
    useFormStore.setState({
      editActorRuntime: (String(actor.runtime || "").trim() || "codex") as SupportedRuntime,
      editActorCommand: formatRuntimeCommand(actor.command),
      editActorTitle: actor.title || "",
      editActorCapabilityAutoloadText: formatCapabilityIdInput(actor.capability_autoload),
    });
    set({ editingActor: actor, editingActorSection: section || null });
  },
  setEditingActor: (actor) =>
    set((state) => ({
      editingActor: actor,
      editingActorSection:
        actor && actor.id === state.editingActor?.id ? state.editingActorSection : null,
    })),
}));
