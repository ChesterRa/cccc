import { create } from "zustand";
import type { RuntimeActivityEvent } from "../types";
import {
  ingestRuntimeActivityEvents,
  pruneRuntimeActivityEvents,
  type RuntimeActivityByGroup,
} from "./runtimeActivityState";

type RuntimeActivityState = {
  byGroup: RuntimeActivityByGroup;
  ingest: (groupId: string, events: RuntimeActivityEvent[]) => void;
  prune: (nowMs?: number) => void;
  clearGroup: (groupId: string) => void;
};

export const useRuntimeActivityStore = create<RuntimeActivityState>((set) => ({
  byGroup: {},
  ingest: (groupId, events) =>
    set((state) => {
      const byGroup = ingestRuntimeActivityEvents(state.byGroup, groupId, events);
      return byGroup === state.byGroup ? state : { byGroup };
    }),
  prune: (nowMs = Date.now()) =>
    set((state) => {
      const byGroup = pruneRuntimeActivityEvents(state.byGroup, nowMs);
      return byGroup === state.byGroup ? state : { byGroup };
    }),
  clearGroup: (groupId) =>
    set((state) => {
      const gid = String(groupId || "").trim();
      if (!gid || !state.byGroup[gid]) return state;
      const byGroup = { ...state.byGroup };
      delete byGroup[gid];
      return { byGroup };
    }),
}));
