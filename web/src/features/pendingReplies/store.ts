import { createContext } from "react";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { PendingReplyAnswer } from "../../services/api/pendingReplies";
import type { ReplyDraft, StructuredQuestion } from "./types";

type Attempt = {
  clientId: string;
  answer: PendingReplyAnswer;
  question: StructuredQuestion | null;
  phase: "sending" | "uncertain";
};

export type ReplyEntry = {
  draft: ReplyDraft;
  attempt: Attempt | null;
  error: string | null;
  closed: "answered" | "withdrawn" | null;
  confirmed: boolean;
};

export const EMPTY_REPLY_ENTRY: ReplyEntry = {
  draft: { selection: null, text: "" },
  attempt: null,
  error: null,
  closed: null,
  confirmed: false,
};

type PendingReplyStore = {
  entries: Record<string, ReplyEntry>;
  setDraft: (key: string, draft: ReplyDraft) => void;
  setError: (key: string, error: string) => void;
  begin: (
    key: string,
    answer: PendingReplyAnswer,
    newId: () => string,
    question?: StructuredQuestion | null,
  ) => Attempt | null;
  fail: (key: string, clientId: string, error: string, uncertain: boolean) => void;
  complete: (key: string, clientId: string) => void;
  close: (key: string, clientId: string, state: "answered" | "withdrawn") => void;
  clear: (key: string) => void;
  retainGroups: (groupIds: readonly string[]) => void;
  clearAll: () => void;
};

export type PendingReplyStoreApi = StoreApi<PendingReplyStore>;
export const PendingReplyStoreContext = createContext<PendingReplyStoreApi | null>(null);

export function replyKey(groupId: string, eventId: string): string {
  return JSON.stringify([groupId, eventId]);
}

/** In-memory only: drafts and retry identity survive virtual-row unmounts. */
export function createPendingReplyStore(): PendingReplyStoreApi {
  return createStore<PendingReplyStore>((set, get) => ({
    entries: {},
    setDraft: (key, draft) =>
      set((state) => {
        const current = state.entries[key] ?? EMPTY_REPLY_ENTRY;
        if (current.attempt || current.closed) return state;
        return { entries: { ...state.entries, [key]: { ...current, draft, error: null } } };
      }),
    setError: (key, error) =>
      set((state) => {
        const current = state.entries[key] ?? EMPTY_REPLY_ENTRY;
        if (current.attempt || current.closed) return state;
        return { entries: { ...state.entries, [key]: { ...current, error } } };
      }),
    begin: (key, answer, newId, question) => {
      const current = get().entries[key] ?? EMPTY_REPLY_ENTRY;
      if (current.closed) return null;
      if (current.attempt?.phase === "sending") return null;
      if (current.attempt && JSON.stringify(current.attempt.answer) !== JSON.stringify(answer))
        return null;
      const attempt: Attempt = current.attempt
        ? { ...current.attempt, phase: "sending" }
        : { clientId: newId(), answer, question: question ?? null, phase: "sending" };
      set((state) => ({
        entries: { ...state.entries, [key]: { ...current, attempt, error: null } },
      }));
      return attempt;
    },
    fail: (key, clientId, error, uncertain) =>
      set((state) => {
        const current = state.entries[key];
        if (current?.attempt?.clientId !== clientId) return state;
        return {
          entries: {
            ...state.entries,
            [key]: {
              ...current,
              attempt: uncertain ? { ...current.attempt, phase: "uncertain" } : null,
              error,
            },
          },
        };
      }),
    complete: (key, clientId) =>
      set((state) => {
        const current = state.entries[key];
        if (current?.attempt?.clientId !== clientId) return state;
        return {
          entries: {
            ...state.entries,
            [key]: { ...current, attempt: null, error: null, closed: "answered", confirmed: true },
          },
        };
      }),
    close: (key, clientId, closed) =>
      set((state) => {
        const current = state.entries[key];
        if (current?.attempt?.clientId !== clientId) return state;
        return {
          entries: {
            ...state.entries,
            [key]: { ...current, attempt: null, error: null, closed, confirmed: false },
          },
        };
      }),
    clear: (key) =>
      set((state) => {
        if (!state.entries[key] || state.entries[key].attempt) return state;
        const entries = { ...state.entries };
        delete entries[key];
        return { entries };
      }),
    retainGroups: (groupIds) =>
      set((state) => {
        const allowed = new Set(groupIds);
        const entries = Object.fromEntries(
          Object.entries(state.entries).filter(([key]) => {
            try {
              const parsed: unknown = JSON.parse(key);
              return (
                Array.isArray(parsed) && typeof parsed[0] === "string" && allowed.has(parsed[0])
              );
            } catch {
              return false;
            }
          }),
        );
        return Object.keys(entries).length === Object.keys(state.entries).length
          ? state
          : { entries };
      }),
    clearAll: () => set({ entries: {} }),
  }));
}
