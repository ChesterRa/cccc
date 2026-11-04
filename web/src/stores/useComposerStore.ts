// Chat composer state store with per-group draft preservation.
import { create } from "zustand";
import type {
  ComposerGroupMentionToken,
  ComposerAgentMentionToken,
} from "../hooks/composerGroupMentions";
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

function isLocalDestination(state: { activeGroupId: string; destGroupId: string }): boolean {
  return !state.destGroupId || state.destGroupId === state.activeGroupId;
}

export interface GroupDraft {
  /** Explicit delivery destination, which may differ from the draft's owning Group. */
  destGroupId?: string;
  retryCandidate?: RetryCandidate | null;
  composerGroupMentionTokens?: ComposerGroupMentionToken[];
  composerAgentMentionTokens?: ComposerAgentMentionToken[];
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;
  quotedPresentationRef: PresentationMessageRef | null;
  quotedVoiceDocumentRef: VoiceDocumentMessageRef | null;
  messageMode: ComposerMessageMode;
}

/** A complete, page-local send snapshot. Files remain in memory, like ordinary drafts. */
export interface FailedSend extends GroupDraft {
  id: string;
  originGroupId: string;
  destGroupId: string;
  /** An unchanged restored request may reuse its original ID when this key still matches. */
  retryKey: string | null;
}

interface RetryCandidate {
  id: string;
  originGroupId: string;
  retryKey: string;
  text: string;
  files: File[];
}

export interface ComposerState {
  composerGroupMentionTokens: ComposerGroupMentionToken[];
  composerAgentMentionTokens: ComposerAgentMentionToken[];
  setComposerGroupMentionTokens: (
    tokens:
      | ComposerGroupMentionToken[]
      | ((prev: ComposerGroupMentionToken[]) => ComposerGroupMentionToken[]),
  ) => void;
  setComposerAgentMentionTokens: (
    tokens:
      | ComposerAgentMentionToken[]
      | ((prev: ComposerAgentMentionToken[]) => ComposerAgentMentionToken[]),
  ) => void;
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
  failedSendsByGroup: Record<string, FailedSend[]>;
  retryCandidate: RetryCandidate | null;

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
  enqueueFailedSend: (snapshot: FailedSend) => void;
  restoreFailedSend: (groupId: string, id: string) => boolean;
  discardFailedSend: (groupId: string, id: string) => void;
}

/** A recovery must not replace even an unfinished or whitespace-only draft. */
export function hasComposerDraft(
  state: Pick<
    ComposerState,
    | "composerText"
    | "composerFiles"
    | "replyTarget"
    | "quotedPresentationRef"
    | "quotedVoiceDocumentRef"
  >,
): boolean {
  return !!(
    state.composerText.length ||
    state.composerFiles.length ||
    state.replyTarget ||
    state.quotedPresentationRef ||
    state.quotedVoiceDocumentRef
  );
}

export const useComposerStore = create<ComposerState>((set, get) => ({
  composerGroupMentionTokens: [],
  composerAgentMentionTokens: [],
  setComposerGroupMentionTokens: (tokens) =>
    set((s) => ({
      composerGroupMentionTokens:
        typeof tokens === "function" ? tokens(s.composerGroupMentionTokens) : tokens,
    })),
  setComposerAgentMentionTokens: (tokens) =>
    set((s) => ({
      composerAgentMentionTokens:
        typeof tokens === "function" ? tokens(s.composerAgentMentionTokens) : tokens,
    })),
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
  failedSendsByGroup: {},
  retryCandidate: null,

  setComposerText: (text) =>
    set((state) => {
      const next = typeof text === "function" ? text(state.composerText) : text;
      return {
        composerText: next,
        retryCandidate: next === state.composerText ? state.retryCandidate : null,
      };
    }),
  setComposerFiles: (files) =>
    set((state) => ({
      composerFiles: files,
      retryCandidate: files === state.composerFiles ? state.retryCandidate : null,
    })),

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
      return {
        composerFiles: next,
        retryCandidate: next.length === state.composerFiles.length ? state.retryCandidate : null,
      };
    }),

  setToText: (text) =>
    set((state) => {
      const nextText = String(text || "");
      if (state.replyTarget || !state.activeGroupId || !isLocalDestination(state)) {
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
        activeGroupId &&
        !state.replyTarget &&
        isLocalDestination(state) &&
        state.normalToTextByGroup[activeGroupId] === undefined
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
        // Reply destinations are temporary, including cross-group replies.
        ...(state.replyTarget ? { destGroupId: activeGroupId } : {}),
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
  setDestGroupId: (groupId) =>
    set((state) => {
      const destGroupId = String(groupId || "").trim() || state.activeGroupId;
      if (destGroupId === (state.destGroupId || state.activeGroupId)) return { destGroupId };
      return {
        destGroupId,
        // Actor IDs are scoped to a Group. A temporary remote selection must
        // neither inherit nor replace this Group's normal recipients.
        ...(!state.replyTarget
          ? {
              toText:
                destGroupId === state.activeGroupId
                  ? (state.normalToTextByGroup[state.activeGroupId] ?? "")
                  : "",
            }
          : {}),
      };
    }),

  clearComposer: () =>
    set((state) => {
      const activeGroupId = String(state.activeGroupId || "").trim();
      const normalToText =
        state.normalToTextByGroup[activeGroupId] ??
        (state.replyTarget || !isLocalDestination(state) ? "" : state.toText);
      return {
        composerGroupMentionTokens: [],
        composerAgentMentionTokens: [],
        composerText: "",
        composerFiles: [],
        toText: normalToText,
        replyTarget: null,
        quotedPresentationRef: null,
        quotedVoiceDocumentRef: null,
        messageMode: state.preferredMessageMode,
        destGroupId: activeGroupId,
        retryCandidate: null,
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
        state.composerText.length > 0 ||
        state.composerFiles.length > 0 ||
        state.replyTarget ||
        state.quotedPresentationRef ||
        state.quotedVoiceDocumentRef;

      if (hasContent) {
        newDrafts[normalizedFromGroupId] = {
          composerGroupMentionTokens: state.composerGroupMentionTokens,
          composerAgentMentionTokens: state.composerAgentMentionTokens,
          composerText: state.composerText,
          composerFiles: state.composerFiles,
          toText: state.toText,
          destGroupId: state.destGroupId || normalizedFromGroupId,
          retryCandidate: state.retryCandidate,
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
    const normalizedDestGroupId = draft?.destGroupId || normalizedToGroupId;

    const nextToText = draft?.toText ?? state.normalToTextByGroup[normalizedToGroupId] ?? "";
    const nextMessageMode = draft?.replyTarget
      ? normalizeReplyMessageMode(draft.messageMode)
      : draft?.messageMode || state.preferredMessageMode;

    set({
      activeGroupId: normalizedToGroupId,
      drafts: newDrafts,
      composerGroupMentionTokens: draft?.composerGroupMentionTokens || [],
      composerAgentMentionTokens: draft?.composerAgentMentionTokens || [],
      composerText: draft?.composerText || "",
      composerFiles: draft?.composerFiles || [],
      toText: nextToText,
      replyTarget: draft?.replyTarget || null,
      quotedPresentationRef: draft?.quotedPresentationRef || null,
      quotedVoiceDocumentRef: draft?.quotedVoiceDocumentRef || null,
      messageMode: nextMessageMode,
      // A draft retains its explicit destination and recipients across Group switches.
      destGroupId: normalizedDestGroupId,
      retryCandidate: draft?.retryCandidate || null,
    });
  },

  upsertDraft: (groupId, updater) =>
    set((state) => {
      const gid = String(groupId || "").trim();
      if (!gid) return state;
      const previous = state.drafts[gid] || null;
      let nextDraft = updater(previous);
      if (
        nextDraft?.retryCandidate &&
        previous &&
        (nextDraft.composerText !== previous.composerText ||
          nextDraft.composerFiles.length !== previous.composerFiles.length ||
          nextDraft.composerFiles.some((file, index) => file !== previous.composerFiles[index]))
      ) {
        nextDraft = { ...nextDraft, retryCandidate: null };
      }
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

  enqueueFailedSend: (snapshot) =>
    set((state) => {
      const queue = state.failedSendsByGroup[snapshot.originGroupId] || [];
      const index = queue.findIndex((item) => item.id === snapshot.id);
      const next = queue.slice();
      if (index < 0) next.push(snapshot);
      else next[index] = snapshot;
      return {
        failedSendsByGroup: { ...state.failedSendsByGroup, [snapshot.originGroupId]: next },
      };
    }),

  restoreFailedSend: (groupId, id) => {
    const state = get();
    if (state.activeGroupId !== groupId || hasComposerDraft(state)) return false;
    const queue = state.failedSendsByGroup[groupId] || [];
    const snapshot = queue.find((item) => item.id === id);
    if (!snapshot) return false;
    const remaining = queue.filter((item) => item.id !== id);
    const failedSendsByGroup = { ...state.failedSendsByGroup };
    if (remaining.length) failedSendsByGroup[groupId] = remaining;
    else delete failedSendsByGroup[groupId];
    set({
      composerGroupMentionTokens: snapshot.composerGroupMentionTokens || [],
      composerAgentMentionTokens: snapshot.composerAgentMentionTokens || [],
      composerText: snapshot.composerText,
      composerFiles: snapshot.composerFiles,
      toText: snapshot.toText,
      replyTarget: snapshot.replyTarget,
      quotedPresentationRef: snapshot.quotedPresentationRef,
      quotedVoiceDocumentRef: snapshot.quotedVoiceDocumentRef,
      messageMode: snapshot.messageMode,
      destGroupId: snapshot.destGroupId,
      retryCandidate: snapshot.retryKey
        ? {
            id: snapshot.id,
            originGroupId: snapshot.originGroupId,
            retryKey: snapshot.retryKey,
            text: snapshot.composerText,
            files: snapshot.composerFiles,
          }
        : null,
      failedSendsByGroup,
    });
    return true;
  },

  discardFailedSend: (groupId, id) =>
    set((state) => {
      const queue = state.failedSendsByGroup[groupId] || [];
      const remaining = queue.filter((item) => item.id !== id);
      if (queue.length === remaining.length) return state;
      const failedSendsByGroup = { ...state.failedSendsByGroup };
      if (remaining.length) failedSendsByGroup[groupId] = remaining;
      else delete failedSendsByGroup[groupId];
      return { failedSendsByGroup };
    }),
}));
