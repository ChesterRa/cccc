// Global observability settings store (developer mode + terminal buffers).
import { create } from "zustand";
import * as apiClient from "../services/api";
import type { Observability } from "../services/api";

const DEFAULT_SCROLLBACK_LINES = 8000;
const DEFAULT_PTY_BACKLOG_MIB = 10;

interface ObservabilityState {
  loaded: boolean;
  developerMode: boolean;
  logLevel: "INFO" | "DEBUG";
  terminalBacklogMiB: number;
  terminalScrollbackLines: number;

  setFromObs: (obs: Observability) => void;
  load: () => Promise<void>;
}

const _fromObs = (obs: Observability) => {
  const lvl = String(obs.log_level || "INFO").toUpperCase();
  const perActorBytes = Number(obs.terminal_transcript?.per_actor_bytes || 0);
  const scrollbackLines = Number(obs.terminal_ui?.scrollback_lines || 0);
  return {
    loaded: true,
    developerMode: Boolean(obs.developer_mode),
    logLevel: (lvl === "DEBUG" ? "DEBUG" : "INFO") as "INFO" | "DEBUG",
    terminalBacklogMiB: Number.isFinite(perActorBytes) && perActorBytes > 0
      ? Math.max(1, Math.round(perActorBytes / (1024 * 1024)))
      : DEFAULT_PTY_BACKLOG_MIB,
    terminalScrollbackLines: Number.isFinite(scrollbackLines) && scrollbackLines > 0
      ? Math.max(1000, Math.round(scrollbackLines))
      : DEFAULT_SCROLLBACK_LINES,
  };
};

export const useObservabilityStore = create<ObservabilityState>((set) => ({
  loaded: false,
  developerMode: false,
  logLevel: "INFO",
  terminalBacklogMiB: DEFAULT_PTY_BACKLOG_MIB,
  terminalScrollbackLines: DEFAULT_SCROLLBACK_LINES,

  setFromObs: (obs) => {
    set(_fromObs(obs));
  },

  load: async () => {
    try {
      const resp = await apiClient.fetchObservability();
      if (resp.ok && resp.result?.observability) {
        set(_fromObs(resp.result.observability));
        return;
      }
      console.error(
        "Failed to load observability settings:",
        resp.error?.message || resp.error?.code || "unknown error"
      );
    } catch (e) {
      console.error("Failed to load observability settings:", e);
    }
    set({ loaded: true });
  },
}));
