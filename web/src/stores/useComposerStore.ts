// Chat composer state store with per-group draft preservation.
import { create } from "zustand";
import type {
  AssistantVoiceDocument,
  PresentationMessageRef,
  ReplyTarget,
  VoiceDocumentMessageRef,
} from "../types";
import { voiceDocumentRefMatchesDocument } from "../utils/voiceDocumentRefs";
import {
  loadComposerMessageModePreference,
  normalizeComposerMessageMode,
  normalizeReplyMessageMode,
  saveComposerMessageModePreference,
  type ComposerMessageMode,
} from "./composerMessageMode";

export {
  COMPOSER_MESSAGE_MODE_STORAGE_KEY,
  DEFAULT_COMPOSER_MESSAGE_MODE,
  loadComposerMessageModePreference,
  normalizeComposerMessageMode,
  normalizeReplyMessageMode,
  type ComposerMessageMode,
} from "./composerMessageMode";
export {
  getComposerDestGroupDisplayValue,
  getEffectiveComposerDestGroupId,
  isComposerGroupSettled,
} from "./composerGroupRouting";

const initialMessageMode = loadComposerMessageModePreference();

interface GroupDraft {
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;
  quotedPresentationRef: PresentationMessageRef | null;
  quotedVoiceDocumentRef: VoiceDocumentMessageRef | null;
  messageMode: ComposerMessageMode;
}

interface ComposerState {
  activeGroupId: string;
  preferredMessageMode: ComposerMessageMode;
  // Current active state
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;
  quotedPresentationRef: PresentationMessageRef | null;
  quotedVoiceDocumentRef: VoiceDocumentMessageRef | null;
  messageMode: ComposerMessageMode;
  destGroupId: string;

  // Drafts per group (memory only)
  drafts: Record<string, GroupDraft>;
  normalToTextByGroup: Record<string, string>;

  // Actions
  setComposerText: (text: string | ((prev: string) => string)) => void;
  setComposerFiles: (files: File[]) => void;
  appendComposerFiles: (files: File[]) => void;
  setToText: (text: string) => void;
  setReplyToText: (text: string) => void;
  setReplyTarget: (target: ReplyTarget) => void;
  setQuotedPresentationRef: (ref: PresentationMessageRef | null) => void;
  setQuotedVoiceDocumentRef: (ref: VoiceDocumentMessageRef | null) => void;
  clearQuotedVoiceDocumentRefsForDocument: (
    groupId: string,
    document: AssistantVoiceDocument,
  ) => void;
  setMessageMode: (mode: ComposerMessageMode) => void;
  setDestGroupId: (groupId: string) => void;
  clearComposer: () => void;

  // Group switching: save current draft and load new group's draft
  switchGroup: (fromGroupId: string | null, toGroupId: string | null) => void;
  upsertDraft: (groupId: string, updater: (draft: GroupDraft | null) => GroupDraft | null) => void;
  // Clear draft for a specific group
  clearDraft: (groupId: string) => void;
}

export const useComposerStore = create<ComposerState>((set, get) => ({
  activeGroupId: "",
  preferredMessageMode: initialMessageMode,
  composerText: "",
  composerFiles: [],
  toText: "",
  replyTarget: null,
  quotedPresentationRef: null,
  quotedVoiceDocumentRef: null,
  messageMode: initialMessageMode,
  destGroupId: "",
  drafts: {},
  normalToTextByGroup: {},

  setComposerText: (text) =>
    set((state) => ({
      composerText: typeof text === "function" ? text(state.composerText) : text,
    })),
  setComposerFiles: (files) => set({ composerFiles: files }),

  appendComposerFiles: (files) =>
    set((state) => {
      const keyOf = (f: File) => `${f.name}:${f.size}:${f.lastModified}`;
      const seen = new Set(state.composerFiles.map(keyOf));
      const next = state.composerFiles.slice();
      for (const f of files) {
        const k = keyOf(f);
        if (!seen.has(k)) {
          seen.add(k);
          next.push(f);
        }
      }
      return { composerFiles: next };
    }),

  setToText: (text) =>
    set((state) => {
      const nextText = String(text || "");
      if (state.replyTarget || !state.activeGroupId) {
        return { toText: nextText };
      }
      return {
        toText: nextText,
        normalToTextByGroup: { ...state.normalToTextByGroup, [state.activeGroupId]: nextText },
      };
    }),
  setReplyToText: (text) =>
    set((state) => {
      const activeGroupId = String(state.activeGroupId || "").trim();
      const normalToTextByGroup =
        activeGroupId && !state.replyTarget
          ? { ...state.normalToTextByGroup, [activeGroupId]: state.toText }
          : state.normalToTextByGroup;
      return { toText: String(text || ""), normalToTextByGroup };
    }),
  setReplyTarget: (target) =>
    set((state) => {
      if (target) {
        return { replyTarget: target, messageMode: "send" };
      }
      const activeGroupId = String(state.activeGroupId || "").trim();
      const normalToText = activeGroupId ? state.normalToTextByGroup[activeGroupId] : undefined;
      return {
        replyTarget: null,
        toText: normalToText ?? "",
        messageMode: state.preferredMessageMode,
      };
    }),
  setQuotedPresentationRef: (ref) => set({ quotedPresentationRef: ref }),
  setQuotedVoiceDocumentRef: (ref) => set({ quotedVoiceDocumentRef: ref }),
  clearQuotedVoiceDocumentRefsForDocument: (groupId, document) =>
    set((state) => {
      const gid = String(groupId || "").trim();
      if (!gid) return state;
      const activeMatches =
        String(state.activeGroupId || "").trim() === gid &&
        !!state.quotedVoiceDocumentRef &&
        voiceDocumentRefMatchesDocument(state.quotedVoiceDocumentRef, gid, document);
      const draft = state.drafts[gid];
      const draftMatches =
        !!draft?.quotedVoiceDocumentRef &&
        voiceDocumentRefMatchesDocument(draft.quotedVoiceDocumentRef, gid, document);
      if (!activeMatches && !draftMatches) return state;
      return {
        ...(activeMatches ? { quotedVoiceDocumentRef: null } : {}),
        ...(draftMatches
          ? { drafts: { ...state.drafts, [gid]: { ...draft, quotedVoiceDocumentRef: null } } }
          : {}),
      };
    }),
  setMessageMode: (mode) => {
    if (get().replyTarget) {
      set({ messageMode: normalizeReplyMessageMode(mode) });
      return;
    }
    const normalized = normalizeComposerMessageMode(mode);
    saveComposerMessageModePreference(normalized);
    set({ preferredMessageMode: normalized, messageMode: normalized });
  },
  setDestGroupId: (groupId) => set({ destGroupId: String(groupId || "").trim() }),

  clearComposer: () =>
    set((state) => {
      const activeGroupId = String(state.activeGroupId || "").trim();
      return {
        composerText: "",
        composerFiles: [],
        toText: "",
        replyTarget: null,
        quotedPresentationRef: null,
        quotedVoiceDocumentRef: null,
        messageMode: state.preferredMessageMode,
        destGroupId: activeGroupId,
        normalToTextByGroup: {},
      };
    }),

  switchGroup: (fromGroupId, toGroupId) => {
    const state = get();
    const normalizedFromGroupId = String(fromGroupId || "").trim();
    const normalizedToGroupId = String(toGroupId || "").trim();
    if (String(state.activeGroupId || "").trim() === normalizedToGroupId) {
      return;
    }
    const newDrafts = { ...state.drafts };

    // Save current state as draft for the old group (if any content)
    if (normalizedFromGroupId) {
      const hasContent =
        state.composerText.trim() ||
        state.composerFiles.length > 0 ||
        state.replyTarget ||
        state.quotedPresentationRef ||
        state.quotedVoiceDocumentRef;

      if (hasContent) {
        newDrafts[normalizedFromGroupId] = {
          composerText: state.composerText,
          composerFiles: state.composerFiles,
          toText: state.replyTarget ? state.toText : "",
          replyTarget: state.replyTarget,
          quotedPresentationRef: state.quotedPresentationRef,
          quotedVoiceDocumentRef: state.quotedVoiceDocumentRef,
          messageMode: state.messageMode,
        };
      } else {
        delete newDrafts[normalizedFromGroupId];
      }
    }

    // Load draft for the new group
    const draft = normalizedToGroupId ? newDrafts[normalizedToGroupId] : null;
    const normalizedDestGroupId = normalizedToGroupId;

    const nextToText = draft?.replyTarget ? draft.toText : "";
    const nextMessageMode = draft?.replyTarget
      ? normalizeReplyMessageMode(draft.messageMode)
      : draft?.messageMode || state.preferredMessageMode;

    set({
      activeGroupId: normalizedDestGroupId,
      drafts: newDrafts,
      normalToTextByGroup: {},
      composerText: draft?.composerText || "",
      composerFiles: draft?.composerFiles || [],
      toText: nextToText,
      replyTarget: draft?.replyTarget || null,
      quotedPresentationRef: draft?.quotedPresentationRef || null,
      quotedVoiceDocumentRef: draft?.quotedVoiceDocumentRef || null,
      messageMode: nextMessageMode,
      // After switching groups, return delivery to the current group. Cross-group
      // sends must be selected explicitly so restored drafts do not trigger remote fetches.
      destGroupId: normalizedDestGroupId,
    });
  },

  upsertDraft: (groupId, updater) =>
    set((state) => {
      const gid = String(groupId || "").trim();
      if (!gid) return state;
      const nextDraft = updater(state.drafts[gid] || null);
      const drafts = { ...state.drafts };
      if (nextDraft) {
        drafts[gid] = nextDraft;
      } else {
        delete drafts[gid];
      }
      return { drafts };
    }),

  clearDraft: (groupId) => {
    const state = get();
    const newDrafts = { ...state.drafts };
    delete newDrafts[groupId];
    set({ drafts: newDrafts });
  },
}));
