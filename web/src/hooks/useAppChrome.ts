import { useCallback, useEffect, useState } from "react";
import * as api from "../services/api";
import { useGroupStore, useObservabilityStore, useUIStore } from "../stores";
import type { DirSuggestion } from "../types";
import { daemonVersionDifference, type DaemonVersionDifference } from "../utils/daemonVersion";
import { MOBILE_VIEWPORT_MEDIA_QUERY } from "../utils/responsiveLayout";

const DISMISSED_VERSION_KEY = "cccc.daemonVersionMismatch.dismissed";

function readDismissedVersion(): string {
  try {
    return window.localStorage.getItem(DISMISSED_VERSION_KEY) || "";
  } catch {
    return "";
  }
}

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
  webModeKnown: boolean;
  ccccHome: string;
  daemonVersionMismatch: DaemonVersionDifference | null;
  dismissDaemonVersionMismatch: () => void;
  fetchDirSuggestions: () => Promise<void>;
  refreshWebAccessSession: () => Promise<boolean | null>;
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
  const [webModeKnown, setWebModeKnown] = useState(false);
  const [versionDifference, setVersionDifference] = useState<DaemonVersionDifference | null>(null);
  const [dismissedVersionDifference, setDismissedVersionDifference] =
    useState(readDismissedVersion);
  const canAccessGlobalSettings = useUIStore((state) => state.canAccessGlobalSettings);
  const setCanAccessGlobalSettings = useUIStore((state) => state.setCanAccessGlobalSettings);

  const refreshWebAccessSession = useCallback(async () => {
    try {
      const resp = await api.fetchWebAccessSession();
      if (
        !resp.ok &&
        !["unauthorized", "auth_required", "permission_denied"].includes(resp.error.code)
      ) {
        setCanAccessGlobalSettings(null);
        return null;
      }
      const session = resp.ok ? (resp.result?.web_access_session ?? null) : null;
      const allowed =
        Boolean(session) &&
        Boolean(session?.can_access_global_settings ?? !(session?.login_active ?? false));
      setCanAccessGlobalSettings(allowed);
      useObservabilityStore.getState().setRuntimeVisibilityFromSession(session);
      return Boolean(session?.is_admin);
    } catch {
      setCanAccessGlobalSettings(null);
      return null;
    }
  }, [setCanAccessGlobalSettings]);

  useEffect(() => {
    void refreshWebAccessSession();
    const handleFocus = () => {
      void refreshWebAccessSession();
    };
    window.addEventListener("focus", handleFocus);
    return () => window.removeEventListener("focus", handleFocus);
  }, [refreshWebAccessSession]);

  // Run first-screen routing and capability probing once; function reference
  // changes should not replay this bootstrap work.
  useEffect(() => {
    parseUrlDeepLink();
    refreshGroups();
    let stopped = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let latest = 0;
    let modeConfirmed = false;
    const readWebMode = async () => {
      const reading = ++latest;
      const response = await api.fetchPing().catch(() => null);
      if (stopped || reading !== latest) return;
      const difference = response?.ok ? daemonVersionDifference(response.result) : null;
      setVersionDifference((previous) =>
        previous?.webVersion === difference?.webVersion &&
        previous?.daemonVersion === difference?.daemonVersion
          ? previous
          : difference,
      );
      const readOnly = response?.ok ? response.result?.web?.read_only : undefined;
      if (typeof readOnly === "boolean") {
        modeConfirmed = true;
        setWebReadOnly(readOnly);
        setWebModeKnown(true);
      } else if (!modeConfirmed) {
        // Do not expose user-only controls until the mode is confirmed.
        retry = setTimeout(() => {
          retry = undefined;
          void readWebMode();
        }, 5000);
      }
    };
    const refreshOnFocus = () => {
      if (document.visibilityState === "hidden") return;
      if (retry !== undefined) clearTimeout(retry);
      retry = undefined;
      void readWebMode();
    };
    void readWebMode();
    window.addEventListener("focus", refreshOnFocus);
    document.addEventListener("visibilitychange", refreshOnFocus);
    return () => {
      stopped = true;
      if (retry !== undefined) clearTimeout(retry);
      window.removeEventListener("focus", refreshOnFocus);
      document.removeEventListener("visibilitychange", refreshOnFocus);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === DISMISSED_VERSION_KEY || event.key === null)
        setDismissedVersionDifference(readDismissedVersion());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  useEffect(() => {
    const mq = window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY);
    const update = () => setSmallScreen(mq.matches);
    update();
    mq.addEventListener("change", update);
    return () => mq.removeEventListener("change", update);
  }, [setSmallScreen]);

  const ensureRuntimesLoaded = useCallback(async () => {
    const state = useGroupStore.getState();
    if (state.runtimeDetectionStatus === "idle") await state.refreshRuntimes();
  }, []);

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

  const versionDifferenceKey = versionDifference
    ? JSON.stringify([versionDifference.webVersion, versionDifference.daemonVersion])
    : "";
  const dismissDaemonVersionMismatch = useCallback(() => {
    if (!versionDifferenceKey) return;
    try {
      window.localStorage.setItem(DISMISSED_VERSION_KEY, versionDifferenceKey);
    } catch {
      // Without storage, dismissal lasts until this page reloads.
    }
    setDismissedVersionDifference(versionDifferenceKey);
  }, [versionDifferenceKey]);
  return {
    canManageGroups: canAccessGlobalSettings === true,
    webModeKnown,
    ccccHome,
    daemonVersionMismatch:
      versionDifferenceKey && versionDifferenceKey !== dismissedVersionDifference
        ? versionDifference
        : null,
    dismissDaemonVersionMismatch,
    fetchDirSuggestions,
    refreshWebAccessSession,
  };
}
