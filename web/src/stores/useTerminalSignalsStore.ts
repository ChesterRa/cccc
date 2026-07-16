import { create } from "zustand";

export type TerminalSignalKind = "idle_prompt" | "working_output";

export type TerminalSignal = { kind: TerminalSignalKind; updatedAt: number };

type TerminalSignalsState = {
  signals: Record<string, TerminalSignal>;
  setSignal: (groupId: string, actorId: string, signal: TerminalSignal) => void;
  clearSignal: (groupId: string, actorId: string) => void;
};

function buildSignalKey(groupId: string, actorId: string): string {
  return `${String(groupId || "").trim()}::${String(actorId || "").trim()}`;
}

export function getTerminalSignalKey(groupId: string, actorId: string): string {
  return buildSignalKey(groupId, actorId);
}

export const useTerminalSignalsStore = create<TerminalSignalsState>((set) => ({
  signals: {},

  setSignal: (groupId, actorId, signal) =>
    set((state) => {
      const key = buildSignalKey(groupId, actorId);
      if (!key || key === "::") return state;
      const previous = state.signals[key];
      if (previous && previous.kind === signal.kind && previous.updatedAt === signal.updatedAt) {
        return state;
      }
      return { signals: { ...state.signals, [key]: signal } };
    }),

  clearSignal: (groupId, actorId) =>
    set((state) => {
      const key = buildSignalKey(groupId, actorId);
      if (!key || !(key in state.signals)) return state;
      const nextSignals = { ...state.signals };
      delete nextSignals[key];
      return { signals: nextSignals };
    }),
}));
