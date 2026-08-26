import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { GroupMeta, RemoteAccessState, WebAccessSession } from "../../../types";
import { ChevronDownIcon, InfoIcon } from "../../Icons";
import { BodyPortal } from "../../ui/BodyPortal";
import { SelectCombobox } from "../../SelectCombobox";
import { InfoPopover } from "./InfoPopover";
import { ReachMembershipSection } from "./ReachMembershipSection";
import { useMembershipController } from "./useMembershipController";
import { useReachWebLogin } from "./useReachWebLogin";
import { useWebAccessSessionSummaries } from "./useWebAccessSessionSummaries";
import { WebAccessReachabilityActions } from "./WebAccessReachabilityActions";
import * as api from "../../../services/api";
import {
  inputClass,
  labelClass,
  primaryButtonClass,
  preClass,
  secondaryButtonClass,
  settingsDialogBodyClass,
  settingsDialogFooterClass,
  settingsDialogHeaderClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";
import { useModalA11y } from "../../../hooks/useModalA11y";
import { copyTextToClipboard } from "../../../utils/copy";
import {
  httpUrl,
  inferAccessGoal,
  isLoopbackHost,
  isRemoteAccessBlockedByMissingAdminToken,
  isWildcardHost,
  keepsActiveReach,
  type AccessGoal,
} from "./webAccessReachabilityModel";

interface WebAccessTabProps {
  isDark: boolean;
  isActive?: boolean;
  focusReach?: boolean;
  onOpenAccount: () => void;
}

type RestartDialogState = {
  localUrl: string;
  remoteUrl: string | null;
  restartCommand: string;
  standaloneCommand: string;
};

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function statusChipClass(_isDark: boolean, tone: "neutral" | "good" | "warn") {
  if (tone === "good") {
    return "border-emerald-500/30 bg-emerald-500/15 text-emerald-600 dark:text-emerald-400";
  }
  if (tone === "warn") {
    return "border-amber-500/30 bg-amber-500/15 text-amber-600 dark:text-amber-400";
  }
  return "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

function resolveApplyRedirectUrl(
  remote: RemoteAccessState,
  targetLocalUrl?: string | null,
  targetRemoteUrl?: string | null,
): string {
  const configHost =
    String(remote.config?.web_host || remote.diagnostics?.web_host || "").trim() || "127.0.0.1";
  const configPort = remote.config?.web_port || remote.diagnostics?.web_port || 8848;
  const currentHost = String(window.location.hostname || "").trim();
  const publicUrl = String(
    remote.config?.web_public_url || remote.diagnostics?.web_public_url || "",
  ).trim();
  if (publicUrl) return publicUrl;
  if (!isLoopbackHost(currentHost)) {
    const usableRemoteUrl = String(targetRemoteUrl || "").trim();
    if (usableRemoteUrl && !usableRemoteUrl.includes("<your-lan-ip>")) {
      return usableRemoteUrl;
    }
    if (isWildcardHost(configHost)) {
      return httpUrl(currentHost, configPort);
    }
    if (!isLoopbackHost(configHost)) {
      return httpUrl(configHost, configPort);
    }
  }
  const usableLocalUrl = String(targetLocalUrl || "").trim();
  if (usableLocalUrl) return usableLocalUrl;
  const usableRemoteUrl = String(targetRemoteUrl || "").trim();
  if (usableRemoteUrl && !usableRemoteUrl.includes("<your-lan-ip>")) {
    return usableRemoteUrl;
  }
  return httpUrl(isLoopbackHost(configHost) ? "127.0.0.1" : configHost, configPort);
}

export function WebAccessTab({
  isDark,
  isActive = true,
  focusReach = false,
  onOpenAccount,
}: WebAccessTabProps) {
  const { t } = useTranslation("settings");
  const membershipController = useMembershipController(isActive);
  const {
    membership,
    membershipBusy,
    membershipError,
    membershipPollReady,
    reachBusy,
    reachAction,
    refresh: refreshMembership,
    connect: connectMembership,
    poll: pollMembership,
    startReach,
    stopReach,
  } = membershipController;
  const reachSectionRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!isActive || !focusReach) return;
    let innerFrame = 0;
    const outerFrame = window.requestAnimationFrame(() => {
      innerFrame = window.requestAnimationFrame(() => {
        reachSectionRef.current?.scrollIntoView({ block: "start", behavior: "smooth" });
      });
    });
    return () => {
      window.cancelAnimationFrame(outerFrame);
      if (innerFrame) window.cancelAnimationFrame(innerFrame);
    };
  }, [focusReach, isActive]);

  const [remoteState, setRemoteState] = useState<RemoteAccessState | null>(null);
  const [accessTokens, setAccessTokens] = useState<api.AccessTokenEntry[]>([]);
  const [session, setSession] = useState<WebAccessSession | null>(null);
  const [groups, setGroups] = useState<GroupMeta[]>([]);

  const [busy, setBusy] = useState(false);
  const [saveBusy, setSaveBusy] = useState(false);
  const [applyBusy, setApplyBusy] = useState(false);
  const [startBusy, setStartBusy] = useState(false);
  const [stopBusy, setStopBusy] = useState(false);
  const [signOutBusy, setSignOutBusy] = useState(false);
  const [error, setError] = useState("");
  const [hint, setHint] = useState("");
  const createReachWebLogin = useReachWebLogin(setError);

  const [provider, setProvider] = useState<"off" | "manual" | "tailscale" | "reach">("off");
  const [mode, setMode] = useState("tailnet_only");
  const [webHost, setWebHost] = useState("127.0.0.1");
  const [webPort, setWebPort] = useState("8848");
  const [webPublicUrl, setWebPublicUrl] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [selectedAccessGoal, setSelectedAccessGoal] = useState<AccessGoal>("local");

  const [userId, setUserId] = useState("");
  const [customToken, setCustomToken] = useState("");
  const [bootstrapToken, setBootstrapToken] = useState("");
  const [isAdmin, setIsAdmin] = useState(false);
  const [selectedGroups, setSelectedGroups] = useState<Set<string>>(new Set());
  const [createBusy, setCreateBusy] = useState(false);
  const [createError, setCreateError] = useState("");
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [newToken, setNewToken] = useState<string | null>(null);
  const [newTokenAutoBound, setNewTokenAutoBound] = useState(false);
  const [copiedNewToken, setCopiedNewToken] = useState(false);

  const [editingTokenId, setEditingTokenId] = useState<string | null>(null);
  const [editGroups, setEditGroups] = useState<Set<string>>(new Set());
  const [editBusy, setEditBusy] = useState(false);
  const [pendingDeleteTokenId, setPendingDeleteTokenId] = useState<string | null>(null);
  const [deleteBusyTokenId, setDeleteBusyTokenId] = useState<string | null>(null);
  const [copiedTokenId, setCopiedTokenId] = useState<string | null>(null);
  const [restartDialog, setRestartDialog] = useState<RestartDialogState | null>(null);
  const advancedDisclosureRef = useRef<HTMLDivElement | null>(null);

  const accessTokenCount = accessTokens.length;
  const hasAdminToken = accessTokens.some((item) => item.is_admin);
  const knownAccessTokenCount =
    typeof session?.access_token_count === "number" ? session.access_token_count : accessTokenCount;
  const loginActive = Boolean(session?.login_active ?? knownAccessTokenCount > 0);
  const canAccessGlobalSettings = Boolean(session?.can_access_global_settings ?? !loginActive);
  const publicAccessTokenConfigured = hasAdminToken;
  const { accessSummary, currentBrowserSummary } = useWebAccessSessionSummaries(
    knownAccessTokenCount,
    loginActive,
    session,
  );

  const pushHint = (value: string) => {
    setHint(value);
    window.setTimeout(() => setHint(""), 1800);
  };

  const closeRestartDialog = useCallback(() => {
    setRestartDialog(null);
  }, []);

  const resetCreateDraft = useCallback(() => {
    setCreateError("");
    setNewToken(null);
    setNewTokenAutoBound(false);
    setCopiedNewToken(false);
    setUserId("");
    setCustomToken("");
    setBootstrapToken("");
    setIsAdmin(false);
    setSelectedGroups(new Set());
  }, []);

  const openCreateDialog = useCallback(() => {
    resetCreateDraft();
    setCreateDialogOpen(true);
  }, [resetCreateDraft]);

  const closeCreateDialog = useCallback(() => {
    setCreateDialogOpen(false);
    resetCreateDraft();
  }, [resetCreateDraft]);

  const { modalRef: createDialogRef } = useModalA11y(createDialogOpen, closeCreateDialog);
  const { modalRef: restartDialogRef } = useModalA11y(Boolean(restartDialog), closeRestartDialog);

  const load = async () => {
    if (!isActive) return;
    setBusy(true);
    setError("");
    try {
      const sessionResp = await api.fetchWebAccessSession();
      let sessionState: WebAccessSession | null = null;
      if (sessionResp.ok && sessionResp.result?.web_access_session) {
        sessionState = sessionResp.result.web_access_session;
        setSession(sessionState);
      } else {
        setError(sessionResp.error?.message || t("webAccess.loadFailed"));
        return;
      }
      if (sessionState.bootstrap_required) {
        setAccessTokens([]);
        setGroups([]);
        setEditingTokenId(null);
        setPendingDeleteTokenId(null);
        return;
      }
      const [remoteResp, groupsResp] = await Promise.all([
        api.fetchRemoteAccessState(),
        api.fetchGroups(),
      ]);
      if (remoteResp.ok && remoteResp.result?.remote_access) {
        const state = remoteResp.result.remote_access;
        setRemoteState(state);
        setProvider((state.provider as "off" | "manual" | "tailscale" | "reach") || "off");
        setMode(String(state.mode || "tailnet_only"));
        setWebHost(String(state.config?.web_host || state.diagnostics?.web_host || "127.0.0.1"));
        setWebPort(String(state.config?.web_port || state.diagnostics?.web_port || 8848));
        setWebPublicUrl(
          String(state.config?.web_public_url || state.diagnostics?.web_public_url || ""),
        );
      } else if (!remoteResp.ok) {
        setError(remoteResp.error?.message || t("webAccess.loadFailed"));
      }
      if (groupsResp.ok && groupsResp.result?.groups) {
        setGroups(groupsResp.result.groups);
      }
      const canAccessGlobal = Boolean(
        sessionState?.can_access_global_settings ?? !(sessionState?.login_active ?? false),
      );
      if (canAccessGlobal && !sessionState?.bootstrap_required) {
        const tokensResp = await api.fetchAccessTokens();
        if (tokensResp.ok && tokensResp.result?.access_tokens) {
          setAccessTokens(tokensResp.result.access_tokens);
        } else if (!tokensResp.ok) {
          setError(tokensResp.error?.message || t("webAccess.loadFailed"));
        }
      } else {
        setAccessTokens([]);
        setEditingTokenId(null);
        setPendingDeleteTokenId(null);
      }
    } catch {
      setError(t("webAccess.loadFailed"));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isActive]);

  useEffect(() => {
    if (!hasAdminToken || isAdmin) {
      setSelectedGroups(new Set());
    }
  }, [hasAdminToken, isAdmin]);

  useEffect(() => {
    if (!canAccessGlobalSettings && !newToken) {
      setCreateDialogOpen(false);
      setNewToken(null);
      setCopiedNewToken(false);
    }
  }, [canAccessGlobalSettings, newToken]);

  const restartRequired = Boolean(remoteState?.restart_required);
  const applySupported = Boolean(
    remoteState?.apply_supported ?? remoteState?.diagnostics?.apply_supported,
  );
  const desiredLocalUrl = String(
    remoteState?.diagnostics?.desired_local_url || httpUrl("127.0.0.1", webPort),
  );
  const desiredRemoteUrl = remoteState?.diagnostics?.desired_remote_url || null;
  const liveBindingHost = remoteState?.diagnostics?.live_runtime_host || null;
  const liveBindingPort = remoteState?.diagnostics?.live_runtime_port || null;
  const liveBindingLabel =
    liveBindingHost && liveBindingPort ? `${liveBindingHost}:${liveBindingPort}` : null;
  const liveLocalUrl = remoteState?.diagnostics?.live_local_url || null;
  const liveRemoteUrl = remoteState?.diagnostics?.live_remote_url || null;
  const liveRuntimePresent = Boolean(remoteState?.diagnostics?.live_runtime_present);
  const lastApplyError = remoteState?.diagnostics?.last_apply_error || null;
  const statusReason = String(remoteState?.status_reason || "");
  const runningInWsl = Boolean(remoteState?.diagnostics?.running_in_wsl);
  const savedProvider = String(remoteState?.provider || "off");
  const savedMode = String(remoteState?.mode || "tailnet_only");
  const savedRequireAccessToken = Boolean(remoteState?.require_access_token ?? true);
  const savedWebHost = String(
    remoteState?.config?.web_host || remoteState?.diagnostics?.web_host || "127.0.0.1",
  );
  const savedWebPort = String(
    remoteState?.config?.web_port || remoteState?.diagnostics?.web_port || 8848,
  );
  const savedWebPublicUrl = String(
    remoteState?.config?.web_public_url || remoteState?.diagnostics?.web_public_url || "",
  );

  const reachabilitySummary = useMemo(() => {
    if (!remoteState || provider === "off" || statusReason === "local_only") {
      return {
        label: t("webAccess.summary.localOnly"),
        detail: t("webAccess.summary.localOnlyHint"),
        tone: "neutral" as const,
      };
    }
    if (restartRequired) {
      return {
        label: t("webAccess.summary.applyPending"),
        detail: applySupported
          ? t("webAccess.summary.applyPendingHint")
          : t("webAccess.summary.applyPendingHintManual"),
        tone: "warn" as const,
      };
    }
    if (statusReason === "missing_access_token") {
      return {
        label: t("webAccess.summary.missingAccessToken"),
        detail: t("webAccess.summary.missingAccessTokenHint"),
        tone: "warn" as const,
      };
    }
    if (statusReason === "binding_unreachable") {
      return {
        label: t("webAccess.summary.bindingUnreachable"),
        detail: t("webAccess.summary.bindingUnreachableHint"),
        tone: "warn" as const,
      };
    }
    if (statusReason === "provider_not_authenticated") {
      return {
        label: t("webAccess.summary.providerAuthRequired"),
        detail: t("webAccess.summary.providerAuthRequiredHint"),
        tone: "warn" as const,
      };
    }
    if (statusReason === "provider_not_installed") {
      return {
        label: t("webAccess.summary.providerNotInstalled"),
        detail: t("webAccess.summary.providerNotInstalledHint"),
        tone: "warn" as const,
      };
    }
    if (remoteState.status === "running") {
      return {
        label: t("webAccess.summary.remoteEnabled"),
        detail:
          savedProvider === "reach"
            ? membership?.hostname || t("webAccess.reach.online")
            : remoteState.endpoint ||
              t("webAccess.summary.remoteEnabledHint", {
                provider: provider === "tailscale" ? "Tailscale" : t("webAccess.providers.manual"),
              }),
        tone: "good" as const,
      };
    }
    return {
      label: t("webAccess.summary.remoteNeedsAttention"),
      detail: t("webAccess.summary.remoteNeedsAttentionHint"),
      tone: "warn" as const,
    };
  }, [
    applySupported,
    membership,
    provider,
    remoteState,
    restartRequired,
    savedProvider,
    statusReason,
    t,
  ]);

  const accessGoal = selectedAccessGoal;
  const remoteMethodValue = provider === "tailscale" ? "tailscale" : "manual";
  const savedAccessGoal = useMemo<AccessGoal>(
    () => inferAccessGoal(savedProvider, savedWebHost, savedWebPublicUrl),
    [savedProvider, savedWebHost, savedWebPublicUrl],
  );

  const exposureClass = useMemo<"local" | "private" | "public">(() => {
    if (webPublicUrl.trim()) return "public";
    if (isLoopbackHost(webHost)) return "local";
    return "private";
  }, [webHost, webPublicUrl]);

  const effectiveRequireAccessToken = exposureClass !== "local";
  const isTailscaleProvider = accessGoal === "lan" && provider === "tailscale";
  const reachabilityDirty = useMemo(() => {
    if (!remoteState) return false;
    return (
      provider !== savedProvider ||
      mode !== savedMode ||
      (accessGoal !== "local" && !savedRequireAccessToken) ||
      webHost.trim() !== savedWebHost.trim() ||
      webPort.trim() !== savedWebPort.trim() ||
      webPublicUrl.trim() !== savedWebPublicUrl.trim()
    );
  }, [
    accessGoal,
    mode,
    provider,
    remoteState,
    savedMode,
    savedProvider,
    savedRequireAccessToken,
    savedWebHost,
    savedWebPort,
    savedWebPublicUrl,
    webHost,
    webPort,
    webPublicUrl,
  ]);
  const primaryReachabilityAction = reachabilityDirty
    ? "save"
    : restartRequired && applySupported
      ? "apply"
      : "idle";
  const draftRemoteAccessBlocked = isRemoteAccessBlockedByMissingAdminToken(
    accessGoal,
    hasAdminToken,
  );
  const savedRemoteAccessBlocked = isRemoteAccessBlockedByMissingAdminToken(
    savedAccessGoal,
    hasAdminToken,
  );
  const actionHintKey = isTailscaleProvider
    ? "webAccess.actionHintTailscale"
    : statusReason === "missing_access_token"
      ? "webAccess.actionHintMissingAccessToken"
      : restartRequired
        ? applySupported
          ? "webAccess.actionHintApplyReady"
          : "webAccess.actionHintManualRestart"
        : "webAccess.actionHintManual";
  useEffect(() => {
    if (provider === "tailscale" || Boolean(lastApplyError)) {
      setShowAdvanced(true);
    }
  }, [lastApplyError, provider]);

  useEffect(() => {
    if (!remoteState) return;
    setSelectedAccessGoal(inferAccessGoal(savedProvider, savedWebHost, savedWebPublicUrl));
  }, [remoteState, savedProvider, savedWebHost, savedWebPublicUrl]);

  const revealAdvancedDisclosure = useCallback(() => {
    window.setTimeout(() => {
      advancedDisclosureRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
    }, 30);
  }, []);

  const toggleAdvanced = useCallback(() => {
    setShowAdvanced((prev) => {
      const next = !prev;
      if (next) {
        revealAdvancedDisclosure();
      }
      return next;
    });
  }, [revealAdvancedDisclosure]);

  const applyAccessGoal = useCallback(
    (goal: AccessGoal) => {
      setSelectedAccessGoal(goal);
      setMode("tailnet_only");
      if (goal === "local") {
        setProvider("off");
        setWebHost("127.0.0.1");
        setWebPublicUrl("");
        return;
      }
      if (goal === "lan") {
        setProvider(provider === "tailscale" ? "tailscale" : "manual");
        setWebHost("0.0.0.0");
        setWebPublicUrl("");
        return;
      }
      setProvider("manual");
      setWebHost("127.0.0.1");
    },
    [provider],
  );

  const handleSignOut = async () => {
    setError("");
    setSignOutBusy(true);
    try {
      const resp = await api.logoutWebAccess();
      if (!resp.ok) {
        setError(resp.error?.message || t("webAccess.loadFailed"));
        return;
      }
    } catch {
      setError(t("webAccess.loadFailed"));
      return;
    } finally {
      api.setForceTokenLogin();
      api.clearAuthToken();
      document.cookie = "cccc_access_token=; path=/; max-age=0";
    }
    setSession((prev) =>
      prev
        ? {
            ...prev,
            current_browser_signed_in: false,
            principal_kind: "anonymous",
            user_id: "",
            is_admin: false,
            allowed_groups: [],
            can_access_global_settings: false,
          }
        : prev,
    );
    pushHint(t("webAccess.signOutSuccess"));
    window.setTimeout(() => {
      window.location.replace(
        window.location.pathname + window.location.search + window.location.hash,
      );
    }, 60);
  };

  const handleSaveReachability = async () => {
    if (draftRemoteAccessBlocked) {
      setError(t("webAccess.remoteAdminTokenRequiredHint"));
      return;
    }
    setSaveBusy(true);
    setError("");
    try {
      const parsedPort = Number.parseInt(webPort, 10);
      if (!Number.isFinite(parsedPort) || parsedPort <= 0 || parsedPort > 65535) {
        setError(t("webAccess.invalidPort"));
        return;
      }
      const trimmedHost = webHost.trim();
      const trimmedPublicUrl = webPublicUrl.trim();
      const keepReach = keepsActiveReach({
        savedProvider,
        draftProvider: provider,
        goal: selectedAccessGoal,
        savedMode,
        draftMode: mode,
        savedHost: savedWebHost,
        draftHost: trimmedHost,
        savedPort: savedWebPort,
        draftPort: String(parsedPort),
        savedPublicUrl: savedWebPublicUrl,
        draftPublicUrl: trimmedPublicUrl,
      });
      if (selectedAccessGoal === "public" && !trimmedPublicUrl && !keepReach) {
        setError(t("webAccess.publicUrlRequired"));
        return;
      }
      if (savedProvider === "reach" && !keepReach) {
        if (!(await stopReach())) return;
      }
      const effectiveProvider =
        selectedAccessGoal === "local"
          ? "off"
          : selectedAccessGoal === "public"
            ? "manual"
            : remoteMethodValue;
      if (
        effectiveProvider === "off" &&
        (!isLoopbackHost(trimmedHost) || Boolean(trimmedPublicUrl))
      ) {
        setError(t("webAccess.offProviderConflict"));
        return;
      }
      const manualEnabled =
        effectiveProvider === "manual" &&
        (!isLoopbackHost(trimmedHost) || Boolean(trimmedPublicUrl));
      const resp = await api.updateRemoteAccessConfig(
        keepReach
          ? { mode, requireAccessToken: true, webHost, webPort: parsedPort }
          : {
              provider: effectiveProvider,
              mode,
              enabled:
                effectiveProvider === "manual"
                  ? manualEnabled
                  : effectiveProvider === "off"
                    ? false
                    : undefined,
              requireAccessToken: true,
              webHost,
              webPort: parsedPort,
              webPublicUrl,
            },
      );
      if (!resp.ok || !resp.result?.remote_access) {
        setError(resp.error?.message || t("webAccess.saveFailed"));
        return;
      }
      setRemoteState(resp.result.remote_access);
      if (resp.result.remote_access.restart_required) {
        if (resp.result.remote_access.apply_supported) {
          pushHint(t("webAccess.applyReady"));
        } else {
          const effectiveHost = String(
            resp.result.remote_access.config?.web_host || trimmedHost || "127.0.0.1",
          );
          const effectivePort = String(resp.result.remote_access.config?.web_port || parsedPort);
          setRestartDialog({
            localUrl: httpUrl("127.0.0.1", effectivePort),
            remoteUrl: isLoopbackHost(effectiveHost)
              ? null
              : httpUrl(
                  isWildcardHost(effectiveHost) ? "<your-lan-ip>" : effectiveHost,
                  effectivePort,
                ),
            restartCommand: "cccc",
            standaloneCommand: "cccc web",
          });
          pushHint(t("webAccess.restartRequired"));
        }
      } else {
        pushHint(t("common:saved"));
      }
    } catch {
      setError(t("webAccess.saveFailed"));
    } finally {
      setSaveBusy(false);
    }
  };

  const handleApplyReachability = async () => {
    if (savedRemoteAccessBlocked) {
      setError(t("webAccess.remoteAdminTokenRequiredHint"));
      return;
    }
    setApplyBusy(true);
    setError("");
    try {
      const resp = await api.applyRemoteAccess();
      if (!resp.ok || !resp.result?.remote_access) {
        setError(resp.error?.message || t("webAccess.applyFailed"));
        return;
      }
      setRemoteState(resp.result.remote_access);
      if (!resp.result.accepted) {
        pushHint(t("webAccess.applyNotNeeded"));
        return;
      }
      pushHint(t("webAccess.applying"));
      const targetUrl = resolveApplyRedirectUrl(
        resp.result.remote_access,
        resp.result.target_local_url || desiredLocalUrl,
        resp.result.target_remote_url || desiredRemoteUrl,
      );
      let targetOrigin = "";
      try {
        targetOrigin = targetUrl ? new URL(targetUrl).origin : "";
      } catch {
        targetOrigin = "";
      }
      const sameOrigin = Boolean(targetOrigin) && targetOrigin === window.location.origin;
      if (!sameOrigin && targetUrl) {
        window.setTimeout(() => {
          window.location.replace(targetUrl);
        }, 1500);
        return;
      }
      for (let attempt = 0; attempt < 30; attempt += 1) {
        await sleep(400);
        const ping = await api.fetchPing();
        if (ping.ok) {
          window.location.reload();
          return;
        }
      }
      window.location.reload();
    } catch {
      setError(t("webAccess.applyFailed"));
    } finally {
      setApplyBusy(false);
    }
  };

  const handleStart = async () => {
    setStartBusy(true);
    setError("");
    try {
      const resp = await api.startRemoteAccess();
      if (!resp.ok || !resp.result?.remote_access) {
        setError(resp.error?.message || t("webAccess.startFailed"));
        return;
      }
      setRemoteState(resp.result.remote_access);
      pushHint(t("webAccess.started"));
    } catch {
      setError(t("webAccess.startFailed"));
    } finally {
      setStartBusy(false);
    }
  };

  const handleStop = async () => {
    setStopBusy(true);
    setError("");
    try {
      const resp = await api.stopRemoteAccess();
      if (!resp.ok || !resp.result?.remote_access) {
        setError(resp.error?.message || t("webAccess.stopFailed"));
        return;
      }
      setRemoteState(resp.result.remote_access);
      pushHint(t("webAccess.stopped"));
    } catch {
      setError(t("webAccess.stopFailed"));
    } finally {
      setStopBusy(false);
    }
  };

  const handleCreateAccessToken = async () => {
    const trimmedUserId = userId.trim();
    if (!trimmedUserId) {
      setCreateError(t("webAccess.userIdRequired"));
      return;
    }
    setCreateBusy(true);
    setCreateError("");
    try {
      const effectiveAdmin = !hasAdminToken || isAdmin;
      if (!effectiveAdmin && selectedGroups.size === 0) {
        setCreateError(t("webAccess.groupSelectionRequired"));
        return;
      }
      const allowedGroups = effectiveAdmin ? [] : [...selectedGroups];
      const shouldAdoptCreatedToken =
        knownAccessTokenCount === 0 && !session?.current_browser_signed_in;
      const resp = await api.createAccessToken(
        trimmedUserId,
        effectiveAdmin,
        allowedGroups,
        customToken.trim() || undefined,
        bootstrapToken.trim() || undefined,
      );
      if (!resp.ok || !resp.result?.access_token) {
        setCreateError(resp.error?.message || t("webAccess.createFailed"));
        return;
      }
      const created = resp.result.access_token;
      setNewTokenAutoBound(shouldAdoptCreatedToken);
      if (created.token) {
        setNewToken(created.token);
        setCreateDialogOpen(true);
      }
      if (shouldAdoptCreatedToken && created.token) {
        api.setAuthToken(created.token);
        setSession({
          login_active: true,
          current_browser_signed_in: true,
          principal_kind: "user",
          user_id: trimmedUserId,
          is_admin: effectiveAdmin,
          allowed_groups: allowedGroups,
          access_token_count: Math.max(knownAccessTokenCount + 1, 1),
          can_access_global_settings: true,
        });
      }
      await load();
    } catch {
      setCreateError(t("webAccess.createFailed"));
    } finally {
      setCreateBusy(false);
    }
  };

  const handleDeleteAccessToken = async (tokenId: string) => {
    if (!tokenId) return;
    setDeleteBusyTokenId(tokenId);
    setError("");
    try {
      const resp = await api.deleteAccessToken(tokenId);
      if (!resp.ok) {
        setError(resp.error?.message || t("webAccess.deleteFailed"));
        return;
      }
      setPendingDeleteTokenId(null);
      if (resp.result?.deleted_current_session || !resp.result?.access_tokens_remain) {
        document.cookie = "cccc_access_token=; path=/; max-age=0";
        api.clearAuthToken();
        window.location.reload();
        return;
      }
      await load();
    } catch {
      setError(t("webAccess.deleteFailed"));
    } finally {
      setDeleteBusyTokenId(null);
    }
  };

  const startEdit = (entry: api.AccessTokenEntry) => {
    if (entry.is_admin) return;
    setPendingDeleteTokenId(null);
    setEditingTokenId(entry.token_id || null);
    setEditGroups(new Set(entry.allowed_groups || []));
  };

  const saveEdit = async () => {
    if (!editingTokenId) return;
    if (editGroups.size === 0) {
      setError(t("webAccess.groupSelectionRequired"));
      return;
    }
    setEditBusy(true);
    try {
      const resp = await api.updateAccessToken(editingTokenId, { allowed_groups: [...editGroups] });
      if (!resp.ok) {
        setError(resp.error?.message || t("webAccess.updateFailed"));
        return;
      }
      setEditingTokenId(null);
      setEditGroups(new Set());
      await load();
    } catch {
      setError(t("webAccess.updateFailed"));
    } finally {
      setEditBusy(false);
    }
  };

  const revealAndCopy = async (tokenId: string) => {
    try {
      const resp = await api.revealAccessToken(tokenId);
      if (!resp.ok || !resp.result?.token) {
        setError(resp.error?.message || t("webAccess.revealFailed"));
        return;
      }
      const ok = await copyTextToClipboard(resp.result.token);
      if (ok) {
        setCopiedTokenId(tokenId);
        window.setTimeout(() => setCopiedTokenId(null), 1500);
      } else {
        setError(t("common:copyFailed"));
      }
    } catch {
      setError(t("webAccess.revealFailed"));
    }
  };

  const copyNewToken = async () => {
    if (!newToken) return;
    const ok = await copyTextToClipboard(newToken);
    if (ok) {
      setCopiedNewToken(true);
      window.setTimeout(() => setCopiedNewToken(false), 1500);
    } else {
      setError(t("common:copyFailed"));
    }
  };

  const createDialog =
    (canAccessGlobalSettings || Boolean(newToken)) && createDialogOpen ? (
      <div className="fixed inset-0 z-[1000] flex items-center justify-center p-4 sm:p-6 animate-fade-in">
        <button
          type="button"
          aria-label={t("webAccess.close")}
          onClick={() => closeCreateDialog()}
          className="absolute inset-0 glass-overlay"
        />
        <div
          ref={createDialogRef}
          role="dialog"
          aria-modal="true"
          aria-label={newToken ? t("webAccess.newTokenTitle") : t("webAccess.createAccessToken")}
          className="glass-modal relative z-[1001] flex max-h-[calc(100dvh-2rem)] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] shadow-2xl text-[var(--color-text-primary)]"
        >
          <div className={settingsDialogHeaderClass}>
            <div>
              <h5 className={`text-base font-semibold text-[var(--color-text-primary)]`}>
                {newToken ? t("webAccess.newTokenTitle") : t("webAccess.createAccessToken")}
              </h5>
              <p className={`mt-1 text-xs leading-6 text-[var(--color-text-muted)]`}>
                {newToken ? t("webAccess.newTokenHint") : t("webAccess.accessControlDescription")}
              </p>
            </div>
            <button
              type="button"
              onClick={() => closeCreateDialog()}
              className={`${secondaryButtonClass("sm")} ml-auto`}
            >
              {t("webAccess.close")}
            </button>
          </div>

          {newToken ? (
            <>
              <div className={`${settingsDialogBodyClass} space-y-5`}>
                <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/15 p-4">
                  <div className="break-all rounded-lg border border-emerald-500/20 bg-[var(--color-bg-primary)] px-3 py-3 text-xs font-mono text-emerald-600 dark:text-emerald-400">
                    {newToken}
                  </div>
                  <div className="mt-3 space-y-1 text-xs leading-6 text-emerald-600 dark:text-emerald-400">
                    <div>
                      {newTokenAutoBound
                        ? t("webAccess.newTokenAutoLoginHint")
                        : t("webAccess.newTokenNoAutoLoginHint")}
                    </div>
                    <div>{t("webAccess.newTokenVerifyHint")}</div>
                  </div>
                </div>
              </div>
              <div className={settingsDialogFooterClass}>
                <button
                  type="button"
                  onClick={() => void copyNewToken()}
                  className={primaryButtonClass(false)}
                >
                  {copiedNewToken ? t("webAccess.copied") : t("webAccess.copyToken")}
                </button>
                <button
                  type="button"
                  onClick={() => closeCreateDialog()}
                  className={secondaryButtonClass()}
                >
                  {t("webAccess.close")}
                </button>
              </div>
            </>
          ) : (
            <>
              <div className={`${settingsDialogBodyClass} space-y-4`}>
                <div className="grid gap-3 lg:grid-cols-2">
                  <div className="space-y-3">
                    <div>
                      <label className={labelClass()}>{t("webAccess.userIdLabel")}</label>
                      <input
                        value={userId}
                        onChange={(e) => setUserId(e.target.value)}
                        className={inputClass()}
                        placeholder={t("webAccess.userIdPlaceholder")}
                      />
                    </div>

                    <div>
                      <label className={labelClass()}>{t("webAccess.customTokenLabel")}</label>
                      <input
                        value={customToken}
                        onChange={(e) => setCustomToken(e.target.value)}
                        className={inputClass()}
                        placeholder={t("webAccess.customTokenPlaceholder")}
                      />
                    </div>
                    {session?.bootstrap_required || knownAccessTokenCount === 0 ? (
                      <div>
                        <label className={labelClass()}>{t("webAccess.bootstrapTokenLabel")}</label>
                        <input
                          value={bootstrapToken}
                          onChange={(e) => setBootstrapToken(e.target.value)}
                          className={inputClass()}
                          placeholder={t("webAccess.bootstrapTokenPlaceholder")}
                          autoComplete="one-time-code"
                        />
                        <div className={`mt-1 text-xs text-[var(--color-text-muted)]`}>
                          {t("webAccess.bootstrapTokenHint")}
                        </div>
                      </div>
                    ) : null}
                  </div>

                  <div className="space-y-3">
                    <label
                      className={`flex items-start gap-3 rounded-lg border px-3 py-3 border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]`}
                    >
                      <input
                        type="checkbox"
                        checked={!hasAdminToken || isAdmin}
                        onChange={(e) => setIsAdmin(e.target.checked)}
                        disabled={!hasAdminToken}
                        className="mt-1"
                      />
                      <div>
                        <div className={`text-sm font-medium text-[var(--color-text-primary)]`}>
                          {t("webAccess.adminTokenLabel")}
                        </div>
                        <div className={`mt-1 text-xs text-[var(--color-text-muted)]`}>
                          {!hasAdminToken
                            ? t("webAccess.firstTokenAdminHint")
                            : t("webAccess.adminTokenHint")}
                        </div>
                      </div>
                    </label>
                  </div>
                </div>

                {hasAdminToken && !isAdmin ? (
                  <div
                    className={`mt-4 rounded-lg border p-3 border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]`}
                  >
                    <div className="flex flex-col gap-1">
                      <div className={`text-sm font-medium text-[var(--color-text-primary)]`}>
                        {t("webAccess.groupScopeTitle")}
                      </div>
                      <div className={`text-xs text-[var(--color-text-muted)]`}>
                        {t("webAccess.scopedTokenGroupsHint")}
                      </div>
                    </div>
                    <div className="mt-3 max-h-44 overflow-y-auto scrollbar-subtle space-y-2 pr-1">
                      {groups.length === 0 ? (
                        <div className={`text-xs text-[var(--color-text-muted)]`}>
                          {t("webAccess.noGroups")}
                        </div>
                      ) : (
                        groups.map((group) => {
                          const groupId = group.group_id;
                          return (
                            <label key={groupId} className="flex items-center gap-2 text-sm">
                              <input
                                type="checkbox"
                                checked={selectedGroups.has(groupId)}
                                onChange={(e) => {
                                  setSelectedGroups((prev) => {
                                    const next = new Set(prev);
                                    if (e.target.checked) next.add(groupId);
                                    else next.delete(groupId);
                                    return next;
                                  });
                                }}
                              />
                              <span className="text-[var(--color-text-primary)]">
                                {group.title || groupId}
                              </span>
                            </label>
                          );
                        })
                      )}
                    </div>
                  </div>
                ) : null}

                {createError && (
                  <div className={`mt-3 text-xs text-red-600 dark:text-red-400`}>{createError}</div>
                )}
              </div>
              <div className={settingsDialogFooterClass}>
                <button
                  type="button"
                  onClick={() => void handleCreateAccessToken()}
                  disabled={createBusy}
                  className={primaryButtonClass(createBusy)}
                >
                  {createBusy ? t("webAccess.creating") : t("webAccess.createAccessToken")}
                </button>
                <button
                  type="button"
                  onClick={() => closeCreateDialog()}
                  className={secondaryButtonClass()}
                >
                  {t("webAccess.cancel")}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    ) : null;

  const restartRequiredDialog = restartDialog ? (
    <div className="fixed inset-0 z-[1000] flex items-center justify-center p-4 sm:p-6 animate-fade-in">
      <button
        type="button"
        aria-label={t("webAccess.close")}
        onClick={closeRestartDialog}
        className="absolute inset-0 glass-overlay"
      />
      <div
        ref={restartDialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("webAccess.restartDialogTitle")}
        className="glass-modal relative z-[1001] flex max-h-[calc(100dvh-2rem)] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] shadow-2xl text-[var(--color-text-primary)]"
      >
        <div className={settingsDialogHeaderClass}>
          <div>
            <h5 className="text-base font-semibold text-[var(--color-text-primary)]">
              {t("webAccess.restartDialogTitle")}
            </h5>
            <p className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
              {t("webAccess.restartDialogBody")}
            </p>
          </div>
          <button
            type="button"
            onClick={closeRestartDialog}
            className={`${secondaryButtonClass("sm")} ml-auto`}
          >
            {t("webAccess.close")}
          </button>
        </div>

        <div className={`${settingsDialogBodyClass} space-y-5`}>
          <div className="rounded-xl border border-amber-500/30 bg-amber-500/12 px-4 py-3 text-xs leading-6 text-[var(--color-text-secondary)]">
            {t("webAccess.restartDialogWhy")}
          </div>

          <div className="grid gap-3 lg:grid-cols-2">
            <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4">
              <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
                {t("webAccess.restartDialogTargetTitle")}
              </div>
              <div className="mt-3 space-y-3 text-sm text-[var(--color-text-secondary)]">
                <div>
                  <div className="text-xs font-medium text-[var(--color-text-muted)]">
                    {t("webAccess.restartDialogLocalLabel")}
                  </div>
                  <div className="mt-1 break-all text-[var(--color-text-primary)]">
                    {restartDialog.localUrl}
                  </div>
                </div>
                {restartDialog.remoteUrl ? (
                  <div>
                    <div className="text-xs font-medium text-[var(--color-text-muted)]">
                      {t("webAccess.restartDialogRemoteLabel")}
                    </div>
                    <div className="mt-1 break-all text-[var(--color-text-primary)]">
                      {restartDialog.remoteUrl}
                    </div>
                  </div>
                ) : null}
              </div>
              <div className="mt-3 text-xs leading-6 text-[var(--color-text-muted)]">
                {t("webAccess.restartDialogLoginHint")}
              </div>
            </div>

            <div className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4">
              <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
                {t("webAccess.restartDialogCommandsTitle")}
              </div>
              <div className="mt-3 space-y-3">
                <div>
                  <div className="mb-2 text-xs font-medium text-[var(--color-text-muted)]">
                    {t("webAccess.restartDialogCommandMainLabel")}
                  </div>
                  <pre className={preClass()}>{restartDialog.restartCommand}</pre>
                  <button
                    type="button"
                    onClick={async () => {
                      const ok = await copyTextToClipboard(restartDialog.restartCommand);
                      if (ok) pushHint(t("webAccess.copied"));
                    }}
                    className={`${secondaryButtonClass("sm")} mt-2`}
                  >
                    {t("webAccess.copyCommand")}
                  </button>
                </div>
                <div>
                  <div className="mb-2 text-xs font-medium text-[var(--color-text-muted)]">
                    {t("webAccess.restartDialogCommandStandaloneLabel")}
                  </div>
                  <pre className={preClass()}>{restartDialog.standaloneCommand}</pre>
                  <button
                    type="button"
                    onClick={async () => {
                      const ok = await copyTextToClipboard(restartDialog.standaloneCommand);
                      if (ok) pushHint(t("webAccess.copied"));
                    }}
                    className={`${secondaryButtonClass("sm")} mt-2`}
                  >
                    {t("webAccess.copyCommand")}
                  </button>
                </div>
              </div>
              <div className="mt-3 text-xs leading-6 text-[var(--color-text-muted)]">
                {t("webAccess.restartDialogSupervisorHint")}
              </div>
            </div>
          </div>
        </div>

        <div className={settingsDialogFooterClass}>
          {restartDialog.remoteUrl ? (
            <button
              type="button"
              onClick={async () => {
                const ok = await copyTextToClipboard(restartDialog.remoteUrl || "");
                if (ok) pushHint(t("webAccess.copied"));
              }}
              className={secondaryButtonClass()}
            >
              {t("webAccess.copyEndpoint")}
            </button>
          ) : null}
          <button type="button" onClick={closeRestartDialog} className={primaryButtonClass(false)}>
            {t("webAccess.restartDialogDone")}
          </button>
        </div>
      </div>
    </div>
  ) : null;

  return (
    <div className="space-y-5">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
                {t("webAccess.title")}
              </h3>
              <InfoPopover
                isDark={isDark}
                title={t("webAccess.howItWorksTitle")}
                content={
                  <ul className="space-y-1">
                    <li>• {t("webAccess.howItWorksActivateLogin")}</li>
                    <li>• {t("webAccess.howItWorksAutoLogin")}</li>
                    <li>• {t("webAccess.howItWorksRemotePolicy")}</li>
                  </ul>
                }
                placement="bottom-start"
                maxWidthClassName="max-w-[320px]"
              >
                {(getReferenceProps, setReference) => (
                  <button
                    type="button"
                    ref={setReference as never}
                    {...getReferenceProps()}
                    className="inline-flex h-7 w-7 items-center justify-center rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] transition-colors"
                    aria-label={t("webAccess.howItWorksTitle")}
                  >
                    <InfoIcon size={14} />
                  </button>
                )}
              </InfoPopover>
            </div>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("webAccess.description")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void Promise.all([load(), refreshMembership()])}
            disabled={
              busy || saveBusy || startBusy || stopBusy || reachBusy || createBusy || editBusy
            }
            className={secondaryButtonClass("sm")}
          >
            {busy ? t("common:loading") : t("webAccess.refresh")}
          </button>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          {(error || hint) && (
            <div
              className={`rounded-lg border px-3 py-2 text-xs ${
                error
                  ? "border-red-500/30 bg-red-500/15 text-red-600 dark:text-red-400"
                  : "border-emerald-500/30 bg-emerald-500/15 text-emerald-600 dark:text-emerald-400"
              }`}
            >
              {error || hint}
            </div>
          )}

          <div className="grid gap-4 lg:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)]">
            <div className={settingsWorkspacePanelClass(isDark)}>
              <div className="text-[11px] uppercase tracking-wide text-[var(--color-text-muted)]">
                {t("webAccess.cards.reachability")}
              </div>
              <div
                className={`mt-2 inline-flex rounded-full border px-2.5 py-1 text-xs ${statusChipClass(isDark, reachabilitySummary.tone)}`}
              >
                {reachabilitySummary.label}
              </div>
              <div className="mt-3 text-sm leading-6 text-[var(--color-text-secondary)]">
                {reachabilitySummary.detail}
              </div>
            </div>

            <div className={settingsWorkspacePanelClass(isDark)}>
              <div className="text-[11px] uppercase tracking-wide text-[var(--color-text-muted)]">
                {t("webAccess.cards.accessControl")}
              </div>
              <div
                className={`mt-2 inline-flex rounded-full border px-2.5 py-1 text-xs ${statusChipClass(isDark, accessSummary.tone)}`}
              >
                {accessSummary.label}
              </div>
              <div className="mt-3 text-sm leading-6 text-[var(--color-text-secondary)]">
                {accessSummary.detail}
              </div>

              <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-[11px] uppercase tracking-wide text-[var(--color-text-muted)]">
                      {t("webAccess.currentBrowserTitle")}
                    </div>
                    <div className="mt-1 text-sm font-medium text-[var(--color-text-primary)]">
                      {currentBrowserSummary.label}
                    </div>
                    <div className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
                      {currentBrowserSummary.detail}
                    </div>
                  </div>
                  <InfoPopover
                    isDark={isDark}
                    title={t("webAccess.currentBrowserTitle")}
                    content={
                      <div className="space-y-2">
                        {loginActive ? <div>{t("webAccess.currentBrowserVerifyHint")}</div> : null}
                        {session?.current_browser_signed_in ? (
                          <div>{t("webAccess.signOutHint")}</div>
                        ) : null}
                      </div>
                    }
                  >
                    {(getReferenceProps, setReference) => (
                      <button
                        type="button"
                        ref={setReference as never}
                        {...getReferenceProps()}
                        className={`inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full border transition-colors ${statusChipClass(isDark, currentBrowserSummary.tone)}`}
                        aria-label={t("webAccess.currentBrowserTitle")}
                      >
                        <InfoIcon size={12} />
                      </button>
                    )}
                  </InfoPopover>
                </div>
                {session?.current_browser_signed_in ? (
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={() => void handleSignOut()}
                      disabled={signOutBusy}
                      className={secondaryButtonClass()}
                    >
                      {signOutBusy ? t("common:loading") : t("webAccess.signOut")}
                    </button>
                  </div>
                ) : null}
              </div>
            </div>
          </div>
        </div>
      </div>

      <details className={`group ${settingsWorkspaceShellClass(isDark)}`}>
        <summary
          className={`${settingsWorkspaceHeaderClass(isDark)} cursor-pointer list-none select-none [&::-webkit-details-marker]:hidden`}
        >
          <div className="min-w-0">
            <h4 className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("webAccess.accessControlTitle")}
            </h4>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("webAccess.accessControlDescription")}
            </p>
          </div>
          <span className="flex shrink-0 items-center gap-2">
            <span
              className={`rounded-full border px-2.5 py-1 text-xs ${statusChipClass(isDark, accessSummary.tone)}`}
            >
              {accessSummary.label}
            </span>
            <ChevronDownIcon
              size={16}
              className="text-[var(--color-text-muted)] transition-transform group-open:rotate-180"
            />
          </span>
        </summary>

        <div className={settingsWorkspaceBodyClass}>
          <div className="space-y-3">
            {canAccessGlobalSettings && knownAccessTokenCount > 0 ? (
              <div className="flex justify-end">
                <button
                  type="button"
                  onClick={() => openCreateDialog()}
                  className={primaryButtonClass(false)}
                >
                  {t("common:create")}
                </button>
              </div>
            ) : null}
            {!canAccessGlobalSettings ? (
              <div className="rounded-lg border border-amber-500/30 bg-amber-500/15 p-6 text-sm text-amber-600 dark:text-amber-400">
                <div className="font-medium">{t("webAccess.managementLockedTitle")}</div>
                <div className="mt-2 text-xs leading-6">{t("webAccess.managementLockedHint")}</div>
              </div>
            ) : accessTokens.length === 0 ? (
              <div className="rounded-xl border border-dashed border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-6 text-[var(--color-text-secondary)]">
                <div className="text-sm font-medium">{t("webAccess.noAccessTokens")}</div>
                <div className="mt-2 text-xs leading-6 text-[var(--color-text-muted)]">
                  {t("webAccess.firstTokenAdminHint")}
                </div>
                <button
                  type="button"
                  onClick={() => openCreateDialog()}
                  className={`mt-4 ${primaryButtonClass(false)}`}
                >
                  {t("common:create")}
                </button>
              </div>
            ) : (
              accessTokens.map((token) => {
                const tokenId = token.token_id || "";
                const isEditing = editingTokenId === tokenId;
                return (
                  <div
                    key={tokenId || token.user_id}
                    className={settingsWorkspacePanelClass(isDark)}
                  >
                    <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                      <div>
                        <div className="flex items-center gap-2 flex-wrap">
                          <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                            {token.user_id}
                          </div>
                          <span
                            className={`rounded-full border px-2 py-0.5 text-[11px] ${statusChipClass(isDark, token.is_admin ? "good" : "neutral")}`}
                          >
                            {token.is_admin
                              ? t("webAccess.adminBadge")
                              : t("webAccess.scopedBadge")}
                          </span>
                          <span
                            className={`rounded-full border px-2 py-0.5 text-[11px] ${statusChipClass(isDark, "neutral")}`}
                          >
                            {token.token_preview || "****"}
                          </span>
                        </div>
                        <div className="mt-1 text-xs text-[var(--color-text-muted)]">
                          {token.is_admin
                            ? t("webAccess.allGroupsAccess")
                            : (token.allowed_groups || []).length > 0
                              ? t("webAccess.scopedGroupsHint", {
                                  count: token.allowed_groups.length,
                                })
                              : t("webAccess.noGroupsAssigned")}
                        </div>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        {!token.is_admin ? (
                          <button
                            type="button"
                            onClick={() => {
                              if (!tokenId) return;
                              startEdit(token);
                            }}
                            disabled={!tokenId || deleteBusyTokenId === tokenId}
                            className={secondaryButtonClass("sm")}
                          >
                            {t("webAccess.edit")}
                          </button>
                        ) : null}
                        <button
                          type="button"
                          onClick={() => void revealAndCopy(tokenId)}
                          disabled={!tokenId || deleteBusyTokenId === tokenId}
                          className={secondaryButtonClass("sm")}
                        >
                          {copiedTokenId === tokenId
                            ? t("webAccess.copied")
                            : t("webAccess.copyToken")}
                        </button>
                        <button
                          type="button"
                          onClick={() => {
                            if (!tokenId) return;
                            setEditingTokenId(null);
                            setPendingDeleteTokenId((prev) => (prev === tokenId ? null : tokenId));
                          }}
                          disabled={!tokenId || deleteBusyTokenId === tokenId}
                          className="inline-flex min-h-[36px] items-center justify-center rounded-xl border border-rose-500/30 bg-rose-500/12 px-3 py-1.5 text-xs font-medium text-rose-600 transition-colors hover:bg-rose-500/18 disabled:opacity-50 dark:text-rose-300"
                        >
                          {t("webAccess.delete")}
                        </button>
                      </div>
                    </div>

                    {pendingDeleteTokenId === tokenId && (
                      <div className="mt-3 rounded-lg border border-red-500/30 bg-red-500/15 p-3">
                        <div className="text-xs text-red-600 dark:text-red-400">
                          {t("webAccess.deleteConfirm")}
                        </div>
                        <div className="mt-3 flex flex-wrap gap-2">
                          <button
                            type="button"
                            onClick={() => void handleDeleteAccessToken(tokenId)}
                            disabled={deleteBusyTokenId === tokenId}
                            className="inline-flex min-h-[36px] items-center justify-center rounded-xl border border-rose-600 bg-rose-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-rose-700 disabled:opacity-50"
                          >
                            {deleteBusyTokenId === tokenId
                              ? t("common:loading")
                              : t("webAccess.delete")}
                          </button>
                          <button
                            type="button"
                            onClick={() => setPendingDeleteTokenId(null)}
                            disabled={deleteBusyTokenId === tokenId}
                            className={secondaryButtonClass("sm")}
                          >
                            {t("webAccess.cancel")}
                          </button>
                        </div>
                      </div>
                    )}

                    {isEditing && (
                      <div className={`mt-3 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                        <div className="text-xs font-medium text-[var(--color-text-secondary)]">
                          {t("webAccess.editScopeTitle")}
                        </div>
                        <div className="mt-2 max-h-44 overflow-y-auto space-y-2 pr-1">
                          {groups.length === 0 ? (
                            <div className="text-xs text-[var(--color-text-muted)]">
                              {t("webAccess.noGroups")}
                            </div>
                          ) : (
                            groups.map((group) => {
                              const groupId = group.group_id;
                              return (
                                <label key={groupId} className="flex items-center gap-2 text-sm">
                                  <input
                                    type="checkbox"
                                    checked={editGroups.has(groupId)}
                                    onChange={(e) => {
                                      setEditGroups((prev) => {
                                        const next = new Set(prev);
                                        if (e.target.checked) next.add(groupId);
                                        else next.delete(groupId);
                                        return next;
                                      });
                                    }}
                                  />
                                  <span className="text-[var(--color-text-primary)]">
                                    {group.title || groupId}
                                  </span>
                                </label>
                              );
                            })
                          )}
                        </div>
                        <div className="mt-3 flex gap-2">
                          <button
                            type="button"
                            onClick={() => void saveEdit()}
                            disabled={editBusy}
                            className={primaryButtonClass(editBusy)}
                          >
                            {editBusy ? t("webAccess.saving") : t("webAccess.save")}
                          </button>
                          <button
                            type="button"
                            onClick={() => {
                              setEditingTokenId(null);
                              setEditGroups(new Set());
                            }}
                            className={secondaryButtonClass("sm")}
                          >
                            {t("webAccess.cancel")}
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>
      </details>

      <section ref={reachSectionRef} className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div>
            <h4 className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("webAccess.reachabilityTitle")}
            </h4>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("webAccess.reachabilityDescription")}
            </p>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <ReachMembershipSection
            membership={membership}
            membershipBusy={membershipBusy}
            membershipError={membershipError}
            membershipPollReady={membershipPollReady}
            hasAdminToken={hasAdminToken}
            reachBusy={reachBusy}
            reachAction={reachAction}
            onConnectAccount={() => void connectMembership()}
            onPollAccount={() => void pollMembership()}
            onOpenAccount={onOpenAccount}
            onCreateAdminToken={openCreateDialog}
            onCreateWebLogin={createReachWebLogin}
            onReachOn={() => {
              void startReach().then((changed) => {
                if (changed) void load();
              });
            }}
            onReachOff={() => {
              void stopReach().then((changed) => {
                if (changed) void load();
              });
            }}
            onCopied={() => pushHint(t("webAccess.copied"))}
            onCopyFailed={() => setError(t("common:copyFailed"))}
          />

          <div className={membership?.online ? "hidden" : settingsWorkspacePanelClass(isDark)}>
            <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
              {t("webAccess.accessGoalTitle")}
            </div>
            <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
              {t("webAccess.accessGoalDescription")}
            </div>
            <div className="mt-3 grid gap-2 xl:grid-cols-3">
              {(["local", "lan", "public"] as const).map((goal) => {
                const active = accessGoal === goal;
                const current = savedAccessGoal === goal;
                return (
                  <button
                    key={goal}
                    type="button"
                    onClick={() => applyAccessGoal(goal)}
                    className={`rounded-xl border px-3 py-3 text-left transition-colors ${
                      active
                        ? "border-emerald-500/35 bg-emerald-500/12"
                        : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] hover:bg-[var(--glass-bg-hover)]"
                    }`}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="text-sm font-medium text-[var(--color-text-primary)]">
                        {t(`webAccess.goals.${goal}.title`)}
                      </div>
                      {current ? (
                        <span className="inline-flex rounded-full border border-emerald-500/30 bg-emerald-500/12 px-2 py-0.5 text-[11px] font-medium text-emerald-600 dark:text-emerald-400">
                          {t("webAccess.goalSelected")}
                        </span>
                      ) : null}
                    </div>
                    <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                      {t(`webAccess.goals.${goal}.hint`)}
                    </div>
                  </button>
                );
              })}
            </div>
          </div>

          <div className={membership?.online ? "hidden" : settingsWorkspacePanelClass(isDark)}>
            <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
              <div>
                <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
                  {t("webAccess.selectedGoalTitle")}
                </div>
                <div className="mt-1 text-sm font-medium text-[var(--color-text-primary)]">
                  {t(`webAccess.goals.${accessGoal}.title`)}
                </div>
                <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                  {t(`webAccess.goals.${accessGoal}.editorHint`)}
                </div>
              </div>
              <div
                className={`inline-flex rounded-full border px-2.5 py-1 text-xs ${statusChipClass(
                  isDark,
                  accessGoal === "public"
                    ? publicAccessTokenConfigured
                      ? "good"
                      : "warn"
                    : "neutral",
                )}`}
              >
                {accessGoal === "public"
                  ? publicAccessTokenConfigured
                    ? t("webAccess.publicTokenProtectedBadge")
                    : t("webAccess.publicTokenRequiredBadge")
                  : accessGoal === "lan"
                    ? t("webAccess.lanGoalBadge")
                    : t("webAccess.localGoalBadge")}
              </div>
            </div>

            {accessGoal === "local" ? (
              <div className="mt-4 max-w-sm">
                <label className={labelClass()}>{t("webAccess.webPortLabel")}</label>
                <input
                  value={webPort}
                  onChange={(e) => setWebPort(e.target.value)}
                  className={inputClass()}
                  inputMode="numeric"
                  placeholder="8848"
                />
                <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                  {t("webAccess.goalPortHintLocal")}
                </div>
              </div>
            ) : null}

            {accessGoal === "lan" ? (
              <>
                <div className="mt-4 grid gap-3 lg:grid-cols-[minmax(0,240px)_minmax(0,240px)]">
                  <div>
                    <label className={labelClass()}>{t("webAccess.providerLabel")}</label>
                    <SelectCombobox
                      items={[
                        { value: "manual", label: t("webAccess.providers.manual") },
                        { value: "tailscale", label: t("webAccess.providers.tailscale") },
                      ]}
                      value={remoteMethodValue}
                      onChange={(value) => {
                        const nextProvider = (value as "manual" | "tailscale") || "manual";
                        setProvider(nextProvider);
                      }}
                      ariaLabel={t("webAccess.providerLabel")}
                      className={inputClass()}
                    />
                    <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                      {remoteMethodValue === "tailscale"
                        ? t("webAccess.modeHintTailscale")
                        : t("webAccess.remoteMethodLanHint")}
                    </div>
                  </div>
                  <div>
                    <label className={labelClass()}>{t("webAccess.webPortLabel")}</label>
                    <input
                      value={webPort}
                      onChange={(e) => setWebPort(e.target.value)}
                      className={inputClass()}
                      inputMode="numeric"
                      placeholder="8848"
                    />
                    <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                      {t("webAccess.goalPortHintLan")}
                    </div>
                  </div>
                </div>
                {remoteMethodValue === "tailscale" ? (
                  <div className="mt-3 rounded-lg border border-black/10 bg-[rgb(245,245,245)] px-3 py-3 text-xs leading-6 text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white">
                    <div className="font-medium">{t("webAccess.privateMethodTailscaleTitle")}</div>
                    <div className="mt-1">{t("webAccess.privateMethodTailscaleHint")}</div>
                  </div>
                ) : null}
                {runningInWsl ? (
                  <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/12 px-3 py-3 text-xs leading-6 text-amber-700 dark:text-amber-300">
                    <div className="font-medium">{t("webAccess.wslHintTitle")}</div>
                    <div className="mt-1">{t("webAccess.wslHintBody")}</div>
                  </div>
                ) : null}
              </>
            ) : null}

            {accessGoal === "public" ? (
              <>
                <div className="mt-4">
                  <label className={labelClass()}>{t("webAccess.webPublicUrlLabel")}</label>
                  <input
                    value={webPublicUrl}
                    onChange={(e) => {
                      const nextValue = e.target.value;
                      setWebPublicUrl(nextValue);
                      if (nextValue.trim()) {
                        setSelectedAccessGoal("public");
                      }
                    }}
                    className={inputClass()}
                    placeholder="https://example.com/ui/"
                  />
                  <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                    {t("webAccess.goalPublicUrlHint")}
                  </div>
                </div>
                <div
                  className={`mt-3 rounded-lg border px-3 py-3 ${
                    publicAccessTokenConfigured
                      ? "border-emerald-500/30 bg-emerald-500/12"
                      : "border-amber-500/30 bg-amber-500/12"
                  }`}
                >
                  <div
                    className={`text-sm font-medium ${
                      publicAccessTokenConfigured
                        ? "text-emerald-700 dark:text-emerald-300"
                        : "text-amber-700 dark:text-amber-300"
                    }`}
                  >
                    {publicAccessTokenConfigured
                      ? t("webAccess.publicTokenProtectedTitle")
                      : t("webAccess.publicTokenRequiredTitle")}
                  </div>
                  <div
                    className={`mt-1 text-xs leading-6 ${
                      publicAccessTokenConfigured
                        ? "text-emerald-700 dark:text-emerald-300"
                        : "text-amber-700 dark:text-amber-300"
                    }`}
                  >
                    {publicAccessTokenConfigured
                      ? t("webAccess.publicTokenProtectedHint")
                      : t("webAccess.publicTokenRequiredHint")}
                  </div>
                </div>
              </>
            ) : null}
          </div>

          {!membership?.online ? (
            <WebAccessReachabilityActions
              action={primaryReachabilityAction}
              actionHint={
                primaryReachabilityAction === "save"
                  ? t("webAccess.primaryActionSaveHint")
                  : t(actionHintKey)
              }
              draftGoal={accessGoal}
              savedGoal={savedAccessGoal}
              hasAdminToken={hasAdminToken}
              saveBusy={saveBusy}
              applyBusy={applyBusy}
              endpoint={membership?.online ? null : remoteState?.endpoint || null}
              onSave={() => void handleSaveReachability()}
              onApply={() => void handleApplyReachability()}
              onCopyEndpoint={async () => {
                const ok = await copyTextToClipboard(remoteState?.endpoint || "");
                if (ok) pushHint(t("webAccess.copied"));
              }}
            />
          ) : null}

          <div ref={advancedDisclosureRef} className={membership?.online ? "hidden" : "mt-4"}>
            <button
              type="button"
              onClick={toggleAdvanced}
              className="w-full rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-4 py-3 text-left transition-colors hover:bg-[var(--glass-bg-hover)]"
              aria-expanded={showAdvanced}
            >
              <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                <div className="min-w-0">
                  <div className="text-sm font-medium text-[var(--color-text-primary)]">
                    {t("webAccess.advancedDisclosureTitle")}
                  </div>
                  <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                    {t("webAccess.advancedDisclosureHint")}
                  </div>
                </div>
                <div className="inline-flex shrink-0 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2.5 py-1 text-xs text-[var(--color-text-secondary)]">
                  {showAdvanced ? t("webAccess.hideAdvanced") : t("webAccess.showAdvanced")}
                </div>
              </div>
            </button>
          </div>

          {showAdvanced && !membership?.online ? (
            <div className={settingsWorkspacePanelClass(isDark)}>
              <div>
                <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
                  {t("webAccess.advancedTitle")}
                </div>
                <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                  {t("webAccess.advancedDescription")}
                </div>
              </div>

              <div className="mt-4 grid gap-3 md:grid-cols-2">
                <div>
                  <label className={labelClass()}>{t("webAccess.webHostLabel")}</label>
                  <input
                    value={webHost}
                    onChange={(e) => {
                      const nextHost = e.target.value;
                      setWebHost(nextHost);
                      if (selectedAccessGoal !== "public" && !webPublicUrl.trim()) {
                        setSelectedAccessGoal(
                          provider === "tailscale"
                            ? "lan"
                            : isLoopbackHost(nextHost)
                              ? "local"
                              : "lan",
                        );
                      }
                    }}
                    className={inputClass()}
                    placeholder="127.0.0.1"
                  />
                  <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                    {t("webAccess.webHostHint")}
                  </div>
                </div>
                {accessGoal === "public" ? (
                  <div>
                    <label className={labelClass()}>{t("webAccess.webPortLabel")}</label>
                    <input
                      value={webPort}
                      onChange={(e) => setWebPort(e.target.value)}
                      className={inputClass()}
                      inputMode="numeric"
                      placeholder="8848"
                    />
                    <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                      {t("webAccess.goalPortHintAdvanced")}
                    </div>
                  </div>
                ) : null}
              </div>

              {accessGoal === "lan" && isTailscaleProvider ? (
                <div className="mt-4 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-3">
                  <div className="text-sm font-medium text-[var(--color-text-primary)]">
                    {t("webAccess.tailscaleTitle")}
                  </div>
                  <div className="mt-1 text-xs leading-6 text-[var(--color-text-muted)]">
                    {t("webAccess.tailscaleDescription")}
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={() => void handleStart()}
                      disabled={startBusy}
                      className={secondaryButtonClass()}
                    >
                      {startBusy ? t("common:loading") : t("webAccess.startTailscale")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleStop()}
                      disabled={stopBusy || remoteState?.enabled !== true}
                      className={secondaryButtonClass()}
                    >
                      {stopBusy ? t("common:loading") : t("webAccess.stopTailscale")}
                    </button>
                  </div>
                </div>
              ) : null}

              {lastApplyError ? (
                <div className="mt-4 rounded-lg border border-amber-500/30 bg-amber-500/12 px-3 py-3 text-xs leading-6 text-amber-700 dark:text-amber-300">
                  <div className="font-medium">{t("webAccess.lastApplyErrorTitle")}</div>
                  <div className="mt-1 break-words">{lastApplyError}</div>
                </div>
              ) : null}

              <div className="mt-4 grid gap-3 lg:grid-cols-[minmax(0,1fr)_300px]">
                <div className="rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3">
                  <div className="text-xs font-medium text-[var(--color-text-secondary)]">
                    {t("webAccess.reachabilityStatusTitle")}
                  </div>
                  <div className="mt-2 text-sm text-[var(--color-text-primary)]">
                    {remoteState?.endpoint || t("webAccess.noEndpoint")}
                  </div>
                  <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-2">
                    <div>
                      <dt className="text-[var(--color-text-muted)]">
                        {t("webAccess.requirementLabel")}
                      </dt>
                      <dd className="text-[var(--color-text-primary)]">
                        {exposureClass === "local"
                          ? t("webAccess.localOnlyPolicy")
                          : effectiveRequireAccessToken
                            ? t("webAccess.required")
                            : t("webAccess.optional")}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-[var(--color-text-muted)]">
                        {t("webAccess.accessTokenPresenceLabel")}
                      </dt>
                      <dd className="text-[var(--color-text-primary)]">
                        {remoteState?.config?.access_token_configured
                          ? t("webAccess.configured")
                          : t("webAccess.notConfigured")}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-[var(--color-text-muted)]">
                        {t("webAccess.savedBindingLabel")}
                      </dt>
                      <dd className="text-[var(--color-text-primary)]">{`${savedWebHost}:${savedWebPort}`}</dd>
                    </div>
                    <div>
                      <dt className="text-[var(--color-text-muted)]">
                        {t("webAccess.liveBindingLabel")}
                      </dt>
                      <dd className="text-[var(--color-text-primary)]">
                        {liveBindingLabel || t("webAccess.notReported")}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-[var(--color-text-muted)]">
                        {t("webAccess.applyStateLabel")}
                      </dt>
                      <dd className="text-[var(--color-text-primary)]">
                        {restartRequired
                          ? t("webAccess.applyStatePending")
                          : liveRuntimePresent
                            ? t("webAccess.applyStateLive")
                            : t("webAccess.notReported")}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-[var(--color-text-muted)]">
                        {t("webAccess.updatedAtLabel")}
                      </dt>
                      <dd className="text-[var(--color-text-primary)]">
                        {remoteState?.updated_at || "-"}
                      </dd>
                    </div>
                  </dl>
                  <div className="mt-3 grid gap-2 text-xs sm:grid-cols-2">
                    <div>
                      <div className="text-[var(--color-text-muted)]">
                        {t("webAccess.localEntryLabel")}
                      </div>
                      <div className="mt-1 break-all text-[var(--color-text-primary)]">
                        {desiredLocalUrl}
                      </div>
                    </div>
                    {desiredRemoteUrl ? (
                      <div>
                        <div className="text-[var(--color-text-muted)]">
                          {t("webAccess.remoteEntryLabel")}
                        </div>
                        <div className="mt-1 break-all text-[var(--color-text-primary)]">
                          {desiredRemoteUrl}
                        </div>
                      </div>
                    ) : null}
                    {liveLocalUrl ? (
                      <div>
                        <div className="text-[var(--color-text-muted)]">
                          {t("webAccess.liveLocalEntryLabel")}
                        </div>
                        <div className="mt-1 break-all text-[var(--color-text-primary)]">
                          {liveLocalUrl}
                        </div>
                      </div>
                    ) : null}
                    {liveRemoteUrl ? (
                      <div>
                        <div className="text-[var(--color-text-muted)]">
                          {t("webAccess.liveRemoteEntryLabel")}
                        </div>
                        <div className="mt-1 break-all text-[var(--color-text-primary)]">
                          {liveRemoteUrl}
                        </div>
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3">
                  <div className="text-xs font-medium text-[var(--color-text-secondary)]">
                    {t("webAccess.nextStepsTitle")}
                  </div>
                  <ul className="mt-2 space-y-2 text-xs text-[var(--color-text-secondary)]">
                    {(remoteState?.next_steps || []).length > 0 ? (
                      (remoteState?.next_steps || []).map((step) => <li key={step}>• {step}</li>)
                    ) : (
                      <li>• {t("webAccess.noNextSteps")}</li>
                    )}
                  </ul>
                </div>
              </div>
            </div>
          ) : null}
        </div>
      </section>

      {createDialog ? <BodyPortal>{createDialog}</BodyPortal> : null}
      {restartRequiredDialog ? <BodyPortal>{restartRequiredDialog}</BodyPortal> : null}
    </div>
  );
}
