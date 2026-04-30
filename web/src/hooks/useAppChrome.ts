import { useCallback, useEffect, useState } from "react";
import * as api from "../services/api";
import { useGroupStore, useObservabilityStore } from "../stores";
import type { DirSuggestion } from "../types";
import { MOBILE_VIEWPORT_MEDIA_QUERY } from "../utils/responsiveLayout";

type UseAppChromeOptions = {
  parseUrlDeepLink: () => void;
  refreshGroups: () => void;
  setWebReadOnly: (value: boolean) => void;
  setSmallScreen: (value: boolean) => void;
  showError: (message: string) => void;
  setDirSuggestions: (suggestions: DirSuggestion[]) => void;
  groupEditOpen: boolean;
  addActorOpen: boolean;
  editingActor: unknown;
};

type UseAppChromeResult = {
  canManageGroups: boolean;
  ccccHome: string;
  fetchDirSuggestions: () => Promise<void>;
};

export function useAppChrome({
  parseUrlDeepLink,
  refreshGroups,
  setWebReadOnly,
  setSmallScreen,
  showError,
  setDirSuggestions,
  groupEditOpen,
  addActorOpen,
  editingActor,
}: UseAppChromeOptions): UseAppChromeResult {
  const [ccccHome, setCcccHome] = useState("");
  const [canAccessGlobalSettings, setCanAccessGlobalSettings] = useState<boolean | null>(null);

  const refreshWebAccessSession = useCallback(async () => {
    try {
      const resp = await api.fetchWebAccessSession();
      const session = resp.ok ? resp.result?.web_access_session ?? null : null;
      const allowed = Boolean(session?.can_access_global_settings ?? !(session?.login_active ?? false));
      setCanAccessGlobalSettings(allowed);
      useObservabilityStore.getState().setRuntimeVisibilityFromSession(session);
    } catch {
      setCanAccessGlobalSettings(null);
    }
  }, []);

  useEffect(() => {
    void refreshWebAccessSession();
    const handleFocus = () => {
      void refreshWebAccessSession();
    };
    window.addEventListener("focus", handleFocus);
    return () => window.removeEventListener("focus", handleFocus);
  }, [refreshWebAccessSession]);

  // 首屏引导和基础能力探测保持一次性执行，避免函数引用变化导致重放。
  useEffect(() => {
    parseUrlDeepLink();
    refreshGroups();
    void api
      .fetchPing()
      .then((resp) => {
        if (resp.ok) {
          setWebReadOnly(Boolean(resp.result?.web?.read_only));
        }
      })
      .catch(() => {
        /* ignore */
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const mq = window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY);
    const update = () => setSmallScreen(mq.matches);
    update();
    mq.addEventListener("change", update);
    return () => mq.removeEventListener("change", update);
  }, [setSmallScreen]);

  const ensureRuntimesLoaded = useCallback(async () => {
    if (useGroupStore.getState().runtimes.length > 0) return;
    try {
      const resp = await api.fetchRuntimes();
      if (resp.ok) {
        useGroupStore.getState().setRuntimes(resp.result.runtimes || []);
        return;
      }
      showError(resp.error?.message || "Failed to load runtimes");
    } catch {
      showError("Failed to load runtimes");
    }
  }, [showError]);

  const fetchDirSuggestions = useCallback(async () => {
    try {
      const resp = await api.fetchDirSuggestions();
      if (resp.ok) {
        setDirSuggestions(resp.result.suggestions || []);
        return;
      }
      showError(resp.error?.message || "Failed to load directories");
    } catch {
      showError("Failed to load directories");
    }
  }, [setDirSuggestions, showError]);

  const loadCcccHome = useCallback(async () => {
    if (ccccHome) return;
    try {
      const resp = await api.fetchPing({ includeHome: true });
      if (resp.ok) {
        setCcccHome(String(resp.result?.home || "").trim());
      }
    } catch {
      /* ignore */
    }
  }, [ccccHome]);

  useEffect(() => {
    if (!groupEditOpen) return;
    void loadCcccHome();
  }, [groupEditOpen, loadCcccHome]);

  useEffect(() => {
    if (!addActorOpen && !editingActor) return;
    void ensureRuntimesLoaded();
  }, [addActorOpen, editingActor, ensureRuntimesLoaded]);

  return {
    canManageGroups: canAccessGlobalSettings === true,
    ccccHome,
    fetchDirSuggestions,
  };
}
