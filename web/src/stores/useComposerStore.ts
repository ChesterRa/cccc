// Composer 状态管理 - 消息编辑器状态
import { create } from "zustand";
import type { ReplyTarget } from "../types";

interface ComposerState {
  composerText: string;
  composerFiles: File[];
  toText: string;
  replyTarget: ReplyTarget;

  // Actions
  setComposerText: (text: string | ((prev: string) => string)) => void;
  setComposerFiles: (files: File[]) => void;
  appendComposerFiles: (files: File[]) => void;
  setToText: (text: string) => void;
  setReplyTarget: (target: ReplyTarget) => void;
  clearComposer: () => void;
}

export const useComposerStore = create<ComposerState>((set) => ({
  composerText: "",
  composerFiles: [],
  toText: "",
  replyTarget: null,

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

  setToText: (text) => set({ toText: text }),
  setReplyTarget: (target) => set({ replyTarget: target }),

  clearComposer: () =>
    set({
      composerText: "",
      composerFiles: [],
      toText: "",
      replyTarget: null,
    }),
}));
