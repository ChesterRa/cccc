import { create } from "zustand";

import * as api from "../services/api";
import type { WebBranding } from "../types";
import {
  applyBrandingToDocument,
  DEFAULT_WEB_BRANDING,
  normalizeWebBranding,
} from "../utils/branding";

interface BrandingState {
  branding: WebBranding;
  loaded: boolean;
  loading: boolean;
  setBranding: (value: Partial<WebBranding> | null | undefined) => void;
  refreshBranding: () => Promise<void>;
}

export const useBrandingStore = create<BrandingState>((set) => ({
  branding: DEFAULT_WEB_BRANDING,
  loaded: false,
  loading: false,
  setBranding: (value) =>
    set((state) => {
      const branding = applyBrandingToDocument(
        normalizeWebBranding({ ...state.branding, ...(value || {}) }),
      );
      return { branding, loaded: true, loading: false };
    }),
  refreshBranding: async () => {
    set({ loading: true });
    try {
      const resp = await api.fetchWebBranding();
      if (!resp.ok) {
        set({ loading: false });
        return;
      }
      const branding = applyBrandingToDocument(normalizeWebBranding(resp.result?.branding));
      set({ branding, loaded: true, loading: false });
    } catch {
      set({ loading: false });
    }
  },
}));
