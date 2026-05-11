// Chat composer state store with per-group draft preservation.
import { create } from "zustand";
import type { PresentationMessageRef, ReplyTarget } from "../types";

export function getEffectiveComposerDestGroupId(
  destGroupId: string,
  activeGroupId: string,
  selectedGroupId: string
): string {
  const selected = String(selectedGroupId || "").trim();
  const active = String(activeGroupId || "").trim();
  const dest = String(destGroupId || "").trim();

  if (!selected) return dest;
  // During the first frame after a group switch, composer state may still belong
  // to the previous group; avoid carrying that old destination into the new group.
  if (active !== selected) return selected;
  return dest || selected;
}

export function isComposerGroupSettled(activeGroupId: string, selectedGroupId: string): boolean {
  return String(activeGroupId || "").trim() === String(selectedGroupId || "").trim();
}

export function getComposerDestGroupDisplayValue(
  destGroupId: string,
  selectedGroupId: string,
  composerGroupSettled: boolean
): string {
  const selected = String(selectedGroupId || "").trim();
  if (!composerGroupSettled) return selected;
  return String(destGroupId || "").trim() || selected;
}

interface GroupDraft {
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;
  quotedPresentationRef: PresentationMessageRef | null;
  priority: "normal" | "attention";
  replyRequired: boolean;
}

interface ComposerState {
  activeGroupId: string;
  // Current active state
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;
  quotedPresentationRef: PresentationMessageRef | null;
  priority: "normal" | "attention";
  replyRequired: boolean;
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
  setPriority: (priority: "normal" | "attention") => void;
  setReplyRequired: (value: boolean) => void;
  setDestGroupId: (groupId: string) => void;
  clearComposer: () => void;

  // Group switching: save current draft and load new group's draft
  switchGroup: (fromGroupId: string | null, toGroupId: string | null) => void;
  upsertDraft: (
    groupId: string,
    updater: (draft: GroupDraft | null) => GroupDraft | null,
  ) => void;
  // Clear draft for a specific group
  clearDraft: (groupId: string) => void;
}

export const useComposerStore = create<ComposerState>((set, get) => ({
  activeGroupId: "",
  composerText: "",
  composerFiles: [],
  toText: "",
  replyTarget: null,
  quotedPresentationRef: null,
  priority: "normal",
  replyRequired: false,
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
        normalToTextByGroup: {
          ...state.normalToTextByGroup,
          [state.activeGroupId]: nextText,
        },
      };
    }),
  setReplyToText: (text) =>
    set((state) => {
      const activeGroupId = String(state.activeGroupId || "").trim();
      const normalToTextByGroup =
        activeGroupId && !state.replyTarget
          ? {
              ...state.normalToTextByGroup,
              [activeGroupId]: state.toText,
            }
          : state.normalToTextByGroup;
      return {
        toText: String(text || ""),
        normalToTextByGroup,
      };
    }),
  setReplyTarget: (target) =>
    set((state) => {
      if (target) {
        return { replyTarget: target };
      }
      const activeGroupId = String(state.activeGroupId || "").trim();
      const normalToText = activeGroupId ? state.normalToTextByGroup[activeGroupId] : undefined;
      return {
        replyTarget: null,
        toText: normalToText ?? state.toText,
      };
    }),
  setQuotedPresentationRef: (ref) => set({ quotedPresentationRef: ref }),
  setPriority: (priority) => set({ priority }),
  setReplyRequired: (value) => set({ replyRequired: !!value }),
  setDestGroupId: (groupId) => set({ destGroupId: String(groupId || "").trim() }),

  clearComposer: () =>
    set((state) => {
      const activeGroupId = String(state.activeGroupId || "").trim();
      const normalToText = activeGroupId ? state.normalToTextByGroup[activeGroupId] : undefined;
      const nextToText = state.replyTarget ? (normalToText ?? "") : state.toText;
      return {
        composerText: "",
        composerFiles: [],
        toText: nextToText,
        replyTarget: null,
        quotedPresentationRef: null,
        priority: "normal",
        replyRequired: false,
        normalToTextByGroup: activeGroupId
          ? {
              ...state.normalToTextByGroup,
              [activeGroupId]: nextToText,
            }
          : state.normalToTextByGroup,
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
        state.toText.trim() ||
        state.replyTarget ||
        state.quotedPresentationRef;

      if (hasContent) {
        newDrafts[normalizedFromGroupId] = {
          composerText: state.composerText,
          composerFiles: state.composerFiles,
          toText: state.toText,
          replyTarget: state.replyTarget,
          quotedPresentationRef: state.quotedPresentationRef,
          priority: state.priority,
          replyRequired: state.replyRequired,
        };
      } else {
        delete newDrafts[normalizedFromGroupId];
      }
    }

    // Load draft for the new group
    const draft = normalizedToGroupId ? newDrafts[normalizedToGroupId] : null;
    const normalizedDestGroupId = normalizedToGroupId;

    const nextToText =
      draft?.toText ?? (normalizedToGroupId ? state.normalToTextByGroup[normalizedToGroupId] : undefined) ?? "";

    set({
      activeGroupId: normalizedDestGroupId,
      drafts: newDrafts,
      composerText: draft?.composerText || "",
      composerFiles: draft?.composerFiles || [],
      toText: nextToText,
      replyTarget: draft?.replyTarget || null,
      quotedPresentationRef: draft?.quotedPresentationRef || null,
      priority: draft?.priority || "normal",
      replyRequired: draft?.replyRequired || false,
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
