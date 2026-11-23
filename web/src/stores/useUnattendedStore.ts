import { create } from "zustand";

import {
  fetchChetUnattended,
  updateChetUnattended,
  type ChetUnattended,
} from "../services/api/chet";

interface UnattendedState {
  /** Unknown until the first read; then the instance's current setting. */
  unattended: ChetUnattended | null;
  saving: boolean;
  error: string;
  refresh: () => Promise<void>;
  setEnabled: (enabled: boolean) => Promise<void>;
}

// Each change bumps the version. A read started before a change, or answered while one is in
// flight, may carry the old setting, so it never replaces what the change stored.
let version = 0;
let changing = 0;

/** Unattended mode belongs to the instance, so the Chet panel and the sidebar entry share one. */
export const useUnattendedStore = create<UnattendedState>((set) => ({
  unattended: null,
  saving: false,
  error: "",
  refresh: async () => {
    const started = version;
    const response = await fetchChetUnattended();
    if (response.ok && changing === 0 && started === version) {
      set({ unattended: response.result, error: "" });
    }
  },
  setEnabled: async (enabled) => {
    version += 1;
    changing += 1;
    set({ saving: true, error: "" });
    try {
      const response = await updateChetUnattended(enabled);
      set(response.ok ? { unattended: response.result } : { error: response.error.message });
    } finally {
      changing -= 1;
      version += 1;
      set({ saving: changing > 0 });
    }
  },
}));
