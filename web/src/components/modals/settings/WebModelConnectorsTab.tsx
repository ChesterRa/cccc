import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, GroupMeta, RemoteAccessState } from "../../../types";
import * as api from "../../../services/api";
import { copyTextToClipboard } from "../../../utils/copy";
import {
  defaultTargetDraftFromSession,
  isChatGptConversationUrl,
  liveBrowserConversationUrlFromSession,
  savedTargetDraftFromSession,
  targetDraftMatchesSaved,
} from "../../../utils/webModelTargetDraft";
import type { TargetDraftMode } from "../../../utils/webModelTargetDraft";
import { ProjectedBrowserSurfacePanel } from "../../browser/ProjectedBrowserSurfacePanel";
import {
  dangerButtonClass,
  inputClass,
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
} from "./types";

interface WebModelConnectorsTabProps {
  isDark: boolean;
  isActive?: boolean;
  currentGroupId?: string;
  onOpenWebAccess?: () => void;
}

const DEFAULT_PROVIDER = "chatgpt_web";
type Translate = (key: string, options?: Record<string, unknown>) => string;

function isLocalConnectorUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    const host = parsed.hostname.toLowerCase();
    return host === "localhost" || host === "127.0.0.1" || host === "::1" || host === "[::1]";
  } catch {
    return false;
  }
}

function isHttpsUrl(url: string): boolean {
  try {
    return new URL(url).protocol === "https:";
  } catch {
    return false;
  }
}

function formatTime(value?: string): string {
  if (!value) return "";
  try {
    return new Date(value).toLocaleString();
  } catch {
    return String(value || "");
  }
}

function connectorUrlWithToken(connectorUrl: string, secret: string): string {
  const url = String(connectorUrl || "").trim();
  const token = String(secret || "").trim();
  if (!url || !token) return "";
  try {
    const parsed = new URL(url);
    parsed.searchParams.set("token", token);
    return parsed.toString();
  } catch {
    const sep = url.includes("?") ? "&" : "?";
    return `${url}${sep}token=${encodeURIComponent(token)}`;
  }
}

function connectorMcpUrl(connector?: api.WebModelConnector | null, secret?: string): string {
  const stored = String(connector?.connector_url_with_token || "").trim();
  if (stored) return stored;
  return connectorUrlWithToken(String(connector?.connector_url || "").trim(), String(secret || "").trim());
}

function connectorActivityLabel(connector: api.WebModelConnector, wm: Translate): string {
  const status = String(connector.last_call_status || "").trim();
  const wait = String(connector.last_wait_status || "").trim();
  const tool = String(connector.last_tool_name || "").trim();
  if (!connector.last_activity_at) return wm("activity.notSeenYet");
  if (status === "error") return wm("activity.lastCallFailed");
  if (tool === "cccc_runtime_wait_next_turn" && wait) return wm("activity.wait", { status: wait });
  if (tool === "cccc_runtime_complete_turn" && wait) return wm("activity.complete", { status: wait });
  return tool || String(connector.last_method || "").trim() || wm("activity.seen");
}

function webModelQueuedCount(actor?: Actor | null): number {
  return Math.max(0, Number(actor?.web_model_queued_count || 0));
}

function isStandardChatGptWebModelActor(actor?: Actor | null): boolean {
  return (
    String(actor?.runtime || "").trim().toLowerCase() === "web_model"
    && !String(actor?.internal_kind || "").trim()
  );
}

function browserSessionKey(groupId: string, actorId: string): string {
  return `${String(groupId || "").trim()}::${String(actorId || "").trim()}`;
}

function shortConversationLabel(url?: string): string {
  const value = String(url || "").trim();
  if (!value) return "";
  try {
    const parsed = new URL(value);
    const parts = parsed.pathname.split("/").filter(Boolean);
    const cIndex = parts.findIndex((part) => part === "c");
    const conversationId = cIndex >= 0 ? parts[cIndex + 1] || "" : "";
    if (conversationId) {
      const prefix = parts.slice(0, cIndex + 1).join("/");
      return `${parsed.hostname}/${prefix}/${conversationId.slice(0, 10)}…`;
    }
    return parsed.hostname;
  } catch {
    return value.length > 48 ? `${value.slice(0, 45)}…` : value;
  }
}

type SetupTone = "ready" | "needs" | "warn" | "neutral";

function setupPillClass(tone: SetupTone): string {
  if (tone === "ready") return "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  if (tone === "needs") return "border-amber-500/25 bg-amber-500/10 text-amber-700 dark:text-amber-200";
  if (tone === "warn") return "border-rose-500/25 bg-rose-500/10 text-rose-700 dark:text-rose-300";
  return "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}

function SetupStatusLine({
  label,
  detail,
  tone,
}: {
  label: string;
  detail: string;
  tone: SetupTone;
}) {
  return (
    <div className="min-w-0 border-t border-[var(--glass-border-subtle)] pt-2 first:border-t-0 first:pt-0 sm:border-t-0 sm:pt-0">
      <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">{label}</div>
      <div className={["mt-1 inline-flex max-w-full rounded-full border px-2 py-0.5 text-xs font-medium", setupPillClass(tone)].join(" ")}>
        <span className="truncate">{detail}</span>
      </div>
    </div>
  );
}

function SetupSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="border-t border-[var(--glass-border-subtle)] pt-3 first:border-t-0 first:pt-0">
      <div className="text-xs font-semibold uppercase tracking-[0.14em] text-[var(--color-text-muted)]">{title}</div>
      <div className="mt-2">{children}</div>
    </div>
  );
}

function healthNextActionText(health: api.WebModelHealthSnapshot | null | undefined, wm: Translate): string {
  const action = health?.next_action;
  const recommended = String(action?.recommended || "none").trim();
  if (!recommended || recommended === "none") return "";
  const label = wm(`nextAction.${recommended}`, { defaultValue: String(action?.label || "").trim() || recommended });
  const reason = String(action?.reason || "").trim();
  return reason ? `${label}: ${reason}` : label;
}

export default function WebModelConnectorsTab({
  isDark,
  isActive = true,
  currentGroupId = "",
  onOpenWebAccess,
}: WebModelConnectorsTabProps) {
  const { t } = useTranslation("settings");
  const wm = useCallback<Translate>((key, options) => t(`webModels.chatgpt.${key}`, options), [t]);
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [actors, setActors] = useState<Actor[]>([]);
  const [connectors, setConnectors] = useState<api.WebModelConnector[]>([]);
  const [remoteState, setRemoteState] = useState<RemoteAccessState | null>(null);
  const [groupId, setGroupId] = useState("");
  const [actorId, setActorId] = useState("");
  const [busy, setBusy] = useState(false);
  const [createBusy, setCreateBusy] = useState(false);
  const [revokeBusyId, setRevokeBusyId] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [browserSession, setBrowserSession] = useState<api.WebModelBrowserSession | null>(null);
  const [browserSessionsByActor, setBrowserSessionsByActor] = useState<Record<string, api.WebModelBrowserSession>>({});
  const [browserBusy, setBrowserBusy] = useState(false);
  const [showBrowserSurface, setShowBrowserSurface] = useState(false);
  const [browserSurfaceRefreshNonce, setBrowserSurfaceRefreshNonce] = useState(0);
  const [browserSurfaceRestartNonce, setBrowserSurfaceRestartNonce] = useState(0);
  const [conversationUrlDraft, setConversationUrlDraft] = useState("");
  const [targetDraftMode, setTargetDraftMode] = useState<TargetDraftMode>("existing");
  const [targetDraftTouched, setTargetDraftTouched] = useState(false);
  const currentSelectionRef = useRef({ groupId: "", actorId: "" });

  useEffect(() => {
    currentSelectionRef.current = { groupId, actorId };
  }, [actorId, groupId]);

  const webModelActors = useMemo(
    () => actors.filter((actor) => isStandardChatGptWebModelActor(actor)),
    [actors],
  );

  const activeConnectors = useMemo(
    () => connectors.filter((connector) => !connector.revoked),
    [connectors],
  );
  const currentGroupActiveConnectors = useMemo(
    () => activeConnectors.filter((connector) => String(connector.group_id || "") === groupId),
    [activeConnectors, groupId],
  );
  const selectedActor = useMemo(
    () => webModelActors.find((actor) => actor.id === actorId) || null,
    [actorId, webModelActors],
  );
  const selectedConnector = useMemo(
    () => {
      if (!selectedActor) return null;
      return currentGroupActiveConnectors.find((connector) => String(connector.actor_id || "") === selectedActor.id) || null;
    },
    [currentGroupActiveConnectors, selectedActor],
  );
  const chatGptActor = webModelActors[0] || null;
  const extraChatGptActors = webModelActors.slice(1);
  const ownerGroup = useMemo(
    () => groups.find((group) => String(group.group_id || "").trim() === groupId) || null,
    [groupId, groups],
  );
  const ownerGroupLabel = String(ownerGroup?.title || ownerGroup?.group_id || "").trim();
  const ownerActorLabel = chatGptActor
    ? String(chatGptActor.title || chatGptActor.id || "").trim()
    : "";
  const configuredPublicUrl = String(remoteState?.config?.web_public_url || remoteState?.diagnostics?.web_public_url || "").trim();
  const publicEndpointReady = Boolean(configuredPublicUrl && isHttpsUrl(configuredPublicUrl));
  const uiAccessTokenPresent = Boolean(remoteState?.config?.access_token_configured || remoteState?.diagnostics?.access_token_present);
  const selectedBrowserSession = browserSessionsByActor[browserSessionKey(groupId, actorId)] || browserSession || null;
  const selectedHealth = selectedBrowserSession?.health_snapshot || null;
  const deliveryTarget = selectedBrowserSession?.delivery_target || selectedHealth?.delivery_target || null;
  const deliveryTargetState = String(deliveryTarget?.state || "").trim();
  const deliveryTargetSavedAt = String(deliveryTarget?.saved_at || selectedBrowserSession?.target_saved_at || selectedHealth?.target?.saved_at || "").trim();
  const browserActive = Boolean(selectedBrowserSession?.active || showBrowserSurface);
  const browserReady = Boolean(selectedBrowserSession?.ready);
  const boundConversationUrl = String(selectedBrowserSession?.conversation_url || "").trim();
  const pendingNewChatBind = Boolean(selectedBrowserSession?.pending_new_chat_bind);
  const pendingNewChatUrl = String(selectedBrowserSession?.pending_new_chat_url || "").trim();
  const liveBrowserUrl = String(selectedBrowserSession?.tab_url || "").trim();
  const currentBrowserUrl = liveBrowserUrl || String(selectedBrowserSession?.last_tab_url || "").trim();
  const currentBrowserConversationUrl = liveBrowserConversationUrlFromSession(selectedBrowserSession);
  const browserStatusLabel = String(selectedHealth?.browser?.label || "").trim() || (browserReady
    ? wm("browser.ready")
    : browserActive
      ? wm("browser.signInNeeded")
      : wm("browser.notOpen"));
  const targetStatusLabel = String(selectedHealth?.target?.label || "").trim() || (boundConversationUrl
    ? wm("target.bound")
    : pendingNewChatBind
      ? wm("target.newChatArmed")
      : wm("target.needed"));
  const savedTargetLabel = boundConversationUrl
    ? wm("target.savedExisting")
    : deliveryTargetState === "new_chat_submitted"
      ? wm("target.savedBinding")
      : pendingNewChatBind
        ? wm("target.savedNewChat")
        : wm("target.savedNone");
  const savedTargetDetail = boundConversationUrl
    ? shortConversationLabel(boundConversationUrl)
    : deliveryTargetState === "new_chat_submitted"
      ? wm("target.bindingDetail")
      : pendingNewChatBind
        ? wm("target.savedNewChatDetail")
        : wm("target.savedNoneDetail");
  const savedTargetTone: SetupTone = boundConversationUrl || pendingNewChatBind ? "ready" : "needs";
  const nextDeliveryDetail = boundConversationUrl
    ? wm("target.nextExisting", { target: shortConversationLabel(boundConversationUrl) })
    : deliveryTargetState === "new_chat_submitted"
      ? wm("target.nextWaitBinding")
      : pendingNewChatBind
        ? wm("target.nextNewChat")
        : wm("target.nextBlocked");
  const currentBrowserDetail = currentBrowserConversationUrl
    ? shortConversationLabel(currentBrowserConversationUrl)
    : currentBrowserUrl
      ? shortConversationLabel(currentBrowserUrl) || currentBrowserUrl
      : wm("target.currentBrowserEmpty");
  const savedTargetDraft = savedTargetDraftFromSession(selectedBrowserSession);
  const targetDraftUrl = String(conversationUrlDraft || "").trim();
  const targetDraftMatchesSavedTarget = targetDraftMatchesSaved({
    mode: targetDraftMode,
    url: targetDraftUrl,
    saved: savedTargetDraft,
  });
  const targetDraftDirty = !targetDraftMatchesSavedTarget;
  const targetDraftError = targetDraftMode === "existing" && targetDraftDirty && !isChatGptConversationUrl(targetDraftUrl)
      ? wm("target.urlInvalid")
      : "";
  const targetUseCurrentAvailable = Boolean(currentBrowserConversationUrl && currentBrowserConversationUrl !== targetDraftUrl);
  const targetSaveDisabled = browserBusy || !groupId || !actorId || Boolean(targetDraftError) || !targetDraftDirty;
  const chooseTargetMode = (mode: TargetDraftMode) => {
    setTargetDraftMode(mode);
    setTargetDraftTouched(true);
  };
  const targetRadioClass = (mode: TargetDraftMode) => [
    "flex cursor-pointer items-start gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors",
    targetDraftMode === mode
      ? "border-[var(--color-text-primary)] bg-[var(--glass-tab-bg-hover)] text-[var(--color-text-primary)]"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
  ].join(" ");
  const selectedActorLabel = selectedActor ? String(selectedActor.title || selectedActor.id || "").trim() : "";
  const selectedActorRunning = Boolean(selectedActor?.running);
  const queuedCount = webModelQueuedCount(selectedActor);
  const selectedMcpUrl = connectorMcpUrl(selectedConnector || null);
  const selectedConnectorUrl = String(selectedConnector?.connector_url || "").trim();
  const selectedMcpUrlForValidation = selectedMcpUrl || selectedConnectorUrl;
  const mcpUrlLocalWarning = Boolean(selectedMcpUrlForValidation) && isLocalConnectorUrl(selectedMcpUrlForValidation);
  const mcpUrlHttpsWarning = Boolean(selectedMcpUrlForValidation) && !isHttpsUrl(selectedMcpUrlForValidation);
  const mcpLastCallFailed = String(selectedConnector?.last_call_status || "").trim() === "error";
  const chatGptSeen = Boolean(selectedConnector?.last_activity_at);
  const mcpStatusLabel = !selectedConnector
    ? wm("mcp.urlNotCreated")
    : !selectedMcpUrl
      ? wm("mcp.needsRotation")
      : mcpLastCallFailed
        ? wm("mcp.lastCallFailed")
        : chatGptSeen
          ? wm("activity.seenAt", { time: formatTime(selectedConnector?.last_activity_at) })
          : wm("mcp.waitingFirstCall");
  const mcpStatusTone: SetupTone = mcpLastCallFailed
    ? "warn"
    : selectedMcpUrl && chatGptSeen
      ? "ready"
      : selectedMcpUrl
        ? "needs"
        : "needs";
  const webAccessReady = publicEndpointReady && uiAccessTokenPresent;
  const actorPrerequisiteLabel = selectedActor
    ? selectedActorRunning
      ? wm("prerequisites.actorReady", { actor: selectedActorLabel || actorId })
      : wm("prerequisites.actorStopped", { actor: selectedActorLabel || actorId })
    : wm("prerequisites.actorMissing");
  const actorPrerequisiteTone: SetupTone = selectedActor ? (selectedActorRunning ? "ready" : "needs") : "needs";
  const browserSummaryTone: SetupTone = browserReady ? "ready" : browserActive ? "needs" : "neutral";
  const webAccessPrerequisiteLabel = webAccessReady
    ? wm("prerequisites.webAccessReady")
    : publicEndpointReady
      ? wm("prerequisites.accessTokenNeeded")
      : wm("prerequisites.publicHttpsNeeded");
  const setupReady = webAccessReady
    && selectedActorRunning
    && browserReady
    && Boolean(selectedMcpUrl)
    && chatGptSeen
    && !mcpLastCallFailed
    && Boolean(boundConversationUrl || pendingNewChatBind);
  const runtimeStatus = setupReady
    ? { label: wm("summary.ready"), tone: "ready" as const }
    : mcpLastCallFailed
      ? { label: wm("summary.needsAttention"), tone: "warn" as const }
      : selectedMcpUrl && !chatGptSeen
        ? { label: wm("summary.waitingForMcp"), tone: "needs" as const }
        : { label: wm("summary.needsSetup"), tone: "needs" as const };
  const nextSetupAction = !publicEndpointReady
    ? wm("next.setPublicHttps")
    : !uiAccessTokenPresent
      ? wm("next.createAccessToken")
      : !selectedActor
        ? wm("next.createActorInGroup")
        : !selectedActorRunning
          ? wm("next.startActorInGroup")
          : !browserReady
            ? wm("next.signIn")
            : !selectedMcpUrl
              ? selectedConnector
                ? wm("next.rotateConnector")
                : wm("next.createConnector")
              : mcpLastCallFailed
                ? wm("next.inspectMcpError")
                : !chatGptSeen
                  ? wm("next.pasteMcpUrl")
                  : !boundConversationUrl && !pendingNewChatBind
                    ? wm("next.bindTarget")
                    : healthNextActionText(selectedHealth, wm) || wm("next.ready");
  const mcpInstructionDetail = selectedMcpUrl
    ? wm("mcp.copyReadyHint")
    : selectedConnector
      ? wm("mcp.rotateHint")
      : wm("mcp.createHint");

  useEffect(() => {
    if (targetDraftTouched) return;
    const draft = defaultTargetDraftFromSession(selectedBrowserSession, currentBrowserConversationUrl);
    setTargetDraftMode(draft.mode);
    setConversationUrlDraft(draft.url);
  }, [currentBrowserConversationUrl, selectedBrowserSession, targetDraftTouched]);

  useEffect(() => {
    if (isActive) setTargetDraftTouched(false);
  }, [actorId, groupId, isActive]);

  const pushNotice = useCallback((value: string) => {
    setNotice(value);
    window.setTimeout(() => setNotice(""), 1600);
  }, []);

  const loadConnectors = useCallback(async () => {
    const resp = await api.fetchWebModelConnectors();
    if (resp.ok) {
      setConnectors(resp.result?.connectors || []);
    } else {
      setError(resp.error?.message || wm("errors.loadConnectorFailed"));
    }
  }, [wm]);

  const loadBrowserSession = useCallback(async (gid: string = groupId, aid: string = actorId) => {
    if ((gid && !aid) || (!gid && aid)) {
      setBrowserSession(null);
      return;
    }
    const resp = await api.fetchWebModelBrowserSession(gid, aid, { inspect: false });
    if (resp.ok) {
      const nextSession = resp.result?.browser_session || null;
      const key = browserSessionKey(gid, aid);
      setBrowserSessionsByActor((current) => ({
        ...current,
        [key]: nextSession || {},
      }));
      const currentSelection = currentSelectionRef.current;
      if (gid === currentSelection.groupId && aid === currentSelection.actorId) setBrowserSession(nextSession);
    } else {
      setError(resp.error?.message || wm("errors.loadBrowserSessionFailed"));
    }
  }, [actorId, groupId, wm]);

  const loadBrowserSessionsForActors = useCallback(async (gid: string, rows: Actor[]) => {
    if (!gid || !rows.length) {
      setBrowserSessionsByActor({});
      return;
    }
    const entries = await Promise.all(
      rows.map(async (actor) => {
        const aid = String(actor.id || "").trim();
        if (!aid) return null;
        const resp = await api.fetchWebModelBrowserSession(gid, aid, { inspect: false });
        return [aid, resp.ok ? resp.result?.browser_session || {} : {}] as const;
      }),
    );
    const next: Record<string, api.WebModelBrowserSession> = {};
    for (const entry of entries) {
      if (entry) next[browserSessionKey(gid, entry[0])] = entry[1];
    }
    if (currentSelectionRef.current.groupId !== gid) return;
    setBrowserSessionsByActor(next);
    const selectedActorId = currentSelectionRef.current.actorId;
    const selectedKey = browserSessionKey(gid, selectedActorId);
    if (selectedActorId && next[selectedKey]) setBrowserSession(next[selectedKey]);
    if (selectedActorId && !next[selectedKey]) setBrowserSession(null);
  }, []);

  const loadActorsForGroup = useCallback(async (gid: string) => {
    if (!gid) {
      setActors([]);
      setActorId("");
      return;
    }
    const resp = await api.fetchActors(gid, true, { noCache: true });
    if (resp.ok) {
      const nextActors = resp.result?.actors || [];
      setActors(nextActors);
      setActorId((current) => {
        if (current && nextActors.some((actor) => actor.id === current && isStandardChatGptWebModelActor(actor))) return current;
        return nextActors.find((actor) => isStandardChatGptWebModelActor(actor))?.id || "";
      });
    } else {
      setActors([]);
      setActorId("");
      setError(resp.error?.message || wm("errors.loadActorsFailed"));
    }
  }, [wm]);

  const loadBrowserSurfaceSession = useCallback(async () => {
    const gid = groupId;
    const aid = actorId;
    const resp = await api.fetchWebModelBrowserSurfaceSession(gid, aid, { inspect: true });
    if (resp.ok) {
      const nextSession = resp.result.browser_session || null;
      const key = browserSessionKey(gid, aid);
      setBrowserSessionsByActor((current) => ({
        ...current,
        [key]: nextSession || {},
      }));
      const currentSelection = currentSelectionRef.current;
      if (gid === currentSelection.groupId && aid === currentSelection.actorId) setBrowserSession(nextSession);
    } else {
      setError(resp.error?.message || wm("errors.loadBrowserSessionFailed"));
    }
    return resp;
  }, [actorId, groupId, wm]);

  const startBrowserSurfaceSession = useCallback(async (size: { width: number; height: number }) => {
    const gid = groupId;
    const aid = actorId;
    const resp = await api.openWebModelBrowserSurfaceSession({
      groupId: gid,
      actorId: aid,
      width: size.width,
      height: size.height,
      inspect: true,
    });
    if (resp.ok) {
      const nextSession = resp.result.browser_session || null;
      const key = browserSessionKey(gid, aid);
      setBrowserSessionsByActor((current) => ({
        ...current,
        [key]: nextSession || {},
      }));
      const currentSelection = currentSelectionRef.current;
      if (gid === currentSelection.groupId && aid === currentSelection.actorId) setBrowserSession(nextSession);
    } else {
      setError(resp.error?.message || wm("errors.openBrowserFailed"));
    }
    return resp;
  }, [actorId, groupId, wm]);

  const loadInitial = useCallback(async () => {
    if (!isActive) return;
    setBusy(true);
    setError("");
    try {
      const [groupsResp, remoteResp] = await Promise.all([
        api.fetchGroups(),
        api.fetchRemoteAccessState(),
        loadConnectors(),
      ]);
      if (remoteResp.ok) {
        setRemoteState(remoteResp.result?.remote_access || null);
      }
      if (groupsResp.ok) {
        const nextGroups = groupsResp.result?.groups || [];
        setGroups(nextGroups);
      } else {
        setError(groupsResp.error?.message || wm("errors.loadGroupsFailed"));
      }
    } catch {
      setError(wm("errors.loadSettingsFailed"));
    } finally {
      setBusy(false);
    }
  }, [isActive, loadConnectors, wm]);

  useEffect(() => {
    void loadInitial();
  }, [loadInitial]);

  useEffect(() => {
    if (!isActive || !groups.length) return;
    let cancelled = false;
    const locateExistingActor = async () => {
      const preferredGroupId = String(currentGroupId || "").trim();
      const orderedGroups = preferredGroupId
        ? [
          ...groups.filter((group) => String(group.group_id || "").trim() === preferredGroupId),
          ...groups.filter((group) => String(group.group_id || "").trim() !== preferredGroupId),
        ]
        : groups;
      for (const group of orderedGroups) {
        const gid = String(group.group_id || "").trim();
        if (!gid) continue;
        const resp = await api.fetchActors(gid, true, { noCache: true });
        if (cancelled) return;
        if (!resp.ok) continue;
        const found = (resp.result?.actors || []).find((actor) => isStandardChatGptWebModelActor(actor));
        if (found) {
          setGroupId(gid);
          setActors(resp.result?.actors || []);
          setActorId(String(found.id || "").trim());
          return;
        }
      }
      setGroupId("");
      setActors([]);
      setActorId("");
      setBrowserSession(null);
      setBrowserSessionsByActor({});
      setShowBrowserSurface(false);
    };
    void locateExistingActor();
    return () => {
      cancelled = true;
    };
  }, [currentGroupId, groups, isActive]);

  useEffect(() => {
    if (!isActive || !groupId) {
      setActors([]);
      setActorId("");
      setBrowserSession(null);
      setBrowserSessionsByActor({});
      setShowBrowserSurface(false);
      return;
    }
    let cancelled = false;
    const loadActors = async () => {
      if (cancelled) return;
      await loadActorsForGroup(groupId);
    };
    void loadActors();
    return () => {
      cancelled = true;
    };
  }, [groupId, isActive, loadActorsForGroup]);

  useEffect(() => {
    if (!isActive) {
      setBrowserSession(null);
      setShowBrowserSurface(false);
      return;
    }
    if (!groupId && !actorId) {
      void loadBrowserSession("", "");
      return;
    }
    if (!groupId || !actorId) {
      setBrowserSession(null);
      return;
    }
    void loadBrowserSession(groupId, actorId);
  }, [actorId, groupId, isActive, loadBrowserSession]);

  useEffect(() => {
    if (!isActive || !groupId || !webModelActors.length) {
      setBrowserSessionsByActor({});
      return;
    }
    void loadBrowserSessionsForActors(groupId, webModelActors);
  }, [groupId, isActive, loadBrowserSessionsForActors, webModelActors]);

  useEffect(() => {
    if (!isActive || !groupId || !actorId || !selectedActor) return;
    let cancelled = false;
    const refresh = async () => {
      const gid = groupId;
      const aid = actorId;
      const resp = await api.fetchWebModelBrowserSession(gid, aid, { inspect: true });
      if (cancelled) return;
      if (resp.ok) {
        const nextSession = resp.result?.browser_session || {};
        const key = browserSessionKey(gid, aid);
        setBrowserSessionsByActor((current) => ({
          ...current,
          [key]: nextSession,
        }));
        const currentSelection = currentSelectionRef.current;
        if (gid === currentSelection.groupId && aid === currentSelection.actorId) setBrowserSession(nextSession);
      }
    };
    void refresh();
    const timer = window.setInterval(() => {
      void refresh();
    }, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [actorId, groupId, isActive, selectedActor]);

  const createConnector = async (targetActorId = actorId) => {
    const aid = String(targetActorId || "").trim();
    if (!groupId || !aid) {
      setError(wm("errors.selectActorFirst"));
      return;
    }
    setCreateBusy(true);
    setError("");
    try {
      const targetActor = webModelActors.find((actor) => actor.id === aid);
      const resp = await api.createWebModelConnector({
        groupId,
        actorId: aid,
        provider: DEFAULT_PROVIDER,
        label: String(targetActor?.title || targetActor?.id || aid),
      });
      if (resp.ok) {
        setActorId(aid);
        const replaced = resp.result?.replaced_connector_ids || [];
        pushNotice(replaced.length ? wm("notices.connectorRotated") : wm("notices.connectorCreated"));
        await loadConnectors();
      } else {
        setError(resp.error?.message || wm("errors.createConnectorFailed"));
      }
    } catch {
      setError(wm("errors.createConnectorFailed"));
    } finally {
      setCreateBusy(false);
    }
  };

  const revokeConnector = async (connectorId: string) => {
    const cid = String(connectorId || "").trim();
    if (!cid) return;
    setRevokeBusyId(cid);
    setError("");
    try {
      const resp = await api.revokeWebModelConnector(cid);
      if (resp.ok) {
        await loadConnectors();
      } else {
        setError(resp.error?.message || wm("errors.revokeConnectorFailed"));
      }
    } catch {
      setError(wm("errors.revokeConnectorFailed"));
    } finally {
      setRevokeBusyId("");
    }
  };

  const openBrowserLogin = async () => {
    setError("");
    setShowBrowserSurface(true);
    setBrowserSurfaceRefreshNonce((value) => value + 1);
    pushNotice(wm("notices.signInSurfaceOpened"));
  };

  const checkBrowserSessionStatus = async () => {
    setBrowserBusy(true);
    setError("");
    try {
      if (showBrowserSurface) {
        await loadBrowserSurfaceSession();
      } else {
        await loadBrowserSession();
      }
    } finally {
      setBrowserBusy(false);
    }
  };

  const reloadEmbeddedBrowser = async () => {
    setBrowserBusy(true);
    setError("");
    try {
      const gid = groupId;
      const aid = actorId;
      const resp = await api.closeWebModelBrowserSurfaceSession(gid, aid);
      if (resp.ok) {
        const nextSession = resp.result?.browser_session || null;
        const key = browserSessionKey(gid, aid);
        setBrowserSessionsByActor((current) => ({
          ...current,
          [key]: nextSession || {},
        }));
        const currentSelection = currentSelectionRef.current;
        if (gid === currentSelection.groupId && aid === currentSelection.actorId) setBrowserSession(nextSession);
        setShowBrowserSurface(true);
        setBrowserSurfaceRestartNonce((value) => value + 1);
        pushNotice(wm("notices.browserRestarted"));
      } else {
        setError(resp.error?.message || wm("errors.restartBrowserFailed"));
      }
    } catch {
      setError(wm("errors.restartBrowserFailed"));
    } finally {
      setBrowserBusy(false);
    }
  };

  const closeBrowserSession = async () => {
    setBrowserBusy(true);
    setError("");
    try {
      const gid = groupId;
      const aid = actorId;
      const resp = await api.closeWebModelBrowserSurfaceSession(gid, aid);
      if (resp.ok) {
        const nextSession = resp.result?.browser_session || null;
        const key = browserSessionKey(gid, aid);
        setBrowserSessionsByActor((current) => ({
          ...current,
          [key]: nextSession || {},
        }));
        const currentSelection = currentSelectionRef.current;
        if (gid === currentSelection.groupId && aid === currentSelection.actorId) {
          setBrowserSession(nextSession);
          setShowBrowserSurface(false);
          pushNotice(wm("notices.browserClosed"));
        }
      } else {
        setError(resp.error?.message || wm("errors.closeBrowserFailed"));
      }
    } catch {
      setError(wm("errors.closeBrowserFailed"));
    } finally {
      setBrowserBusy(false);
    }
  };

  const bindConversation = async (conversationUrl = "", options?: { newChat?: boolean; notice?: string }) => {
    if (!groupId || !actorId) return;
    setBrowserBusy(true);
    setError("");
    try {
      const gid = groupId;
      const aid = actorId;
      const resp = await api.bindCurrentWebModelBrowserConversation({
        groupId: gid,
        actorId: aid,
        conversationUrl,
        newChat: Boolean(options?.newChat),
      });
      if (resp.ok) {
        const nextSession = resp.result?.browser_session || null;
        const key = browserSessionKey(gid, aid);
        setBrowserSessionsByActor((current) => ({
          ...current,
          [key]: nextSession || {},
        }));
        const currentSelection = currentSelectionRef.current;
        if (gid === currentSelection.groupId && aid === currentSelection.actorId) {
          setBrowserSession(nextSession);
          const draft = savedTargetDraftFromSession(nextSession);
          setTargetDraftMode(draft.mode);
          setConversationUrlDraft(draft.url);
          setTargetDraftTouched(false);
          pushNotice(options?.notice || (options?.newChat ? wm("notices.newChatSelected") : wm("notices.conversationBound")));
        }
      } else {
        setError(resp.error?.message || wm("errors.bindConversationFailed"));
      }
    } catch {
      setError(wm("errors.bindConversationFailed"));
    } finally {
      setBrowserBusy(false);
    }
  };

  const saveDeliveryTarget = async () => {
    if (targetSaveDisabled) return;
    if (targetDraftMode === "new") {
      await bindConversation("https://chatgpt.com/", {
        newChat: true,
        notice: wm("notices.targetSavedNewChat"),
      });
      return;
    }
    await bindConversation(targetDraftUrl, {
      notice: wm("notices.targetSavedExisting"),
    });
  };

  const copyValue = async (value: string, labelText: string) => {
    const ok = await copyTextToClipboard(value);
    pushNotice(ok ? wm("notices.copied", { label: labelText }) : wm("notices.copyFailed"));
  };

  return (
    <div className={settingsWorkspaceShellClass(isDark)}>
      <div className={settingsWorkspaceHeaderClass(isDark)}>
        <div className="min-w-0">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--color-text-muted)]">
            {wm("header.kicker")}
          </div>
          <h3 className="mt-1 text-base font-semibold text-[var(--color-text-primary)]">{wm("header.title")}</h3>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-[var(--color-text-tertiary)]">
            {wm("header.description")}
          </p>
        </div>
        <button type="button" onClick={() => void loadInitial()} disabled={busy} className={secondaryButtonClass("sm")}>
          {wm("buttons.refresh")}
        </button>
      </div>

      <div className={settingsWorkspaceBodyClass}>
        {error ? (
          <div className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">
            {error}
          </div>
        ) : null}
        {notice ? (
          <div className="rounded-lg border border-emerald-500/25 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-700 dark:text-emerald-300">
            {notice}
          </div>
        ) : null}

        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">{wm("summary.title")}</div>
                <span className={["inline-flex rounded-full border px-2 py-0.5 text-xs font-semibold", setupPillClass(runtimeStatus.tone)].join(" ")}>
                  {runtimeStatus.label}
                </span>
                {queuedCount > 0 ? (
                  <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2 py-0.5 text-[11px] font-medium text-amber-700 dark:text-amber-200">
                    {wm("queue.queued", { count: queuedCount })}
                  </span>
                ) : null}
              </div>
              <div className="mt-2 text-sm leading-6 text-[var(--color-text-secondary)]">
                {wm("next.prefix", { action: nextSetupAction })}
              </div>
            </div>
          </div>
          <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
            <SetupStatusLine label={wm("summary.webAccess")} detail={webAccessPrerequisiteLabel} tone={webAccessReady ? "ready" : "needs"} />
            <SetupStatusLine label={wm("summary.actor")} detail={actorPrerequisiteLabel} tone={actorPrerequisiteTone} />
            <SetupStatusLine label={wm("summary.chatgpt")} detail={browserStatusLabel} tone={browserSummaryTone} />
            <SetupStatusLine label={wm("summary.mcpApp")} detail={mcpStatusLabel} tone={mcpStatusTone} />
            <SetupStatusLine label={wm("summary.deliveryTarget")} detail={savedTargetLabel} tone={savedTargetTone} />
          </div>
        </section>

        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="text-sm font-semibold text-[var(--color-text-primary)]">{wm("prerequisites.title")}</div>
          <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
            {wm("prerequisites.description")}
          </p>
          <div className="mt-4 grid gap-4 lg:grid-cols-2">
            <div className="min-w-0 border-t border-[var(--glass-border-subtle)] pt-3 first:border-t-0 first:pt-0 lg:border-t-0 lg:pt-0">
              <div className="flex flex-wrap items-center gap-2">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">{wm("prerequisites.webAccessStepTitle")}</div>
                <span className={["inline-flex rounded-full border px-2 py-0.5 text-xs font-semibold", setupPillClass(webAccessReady ? "ready" : "needs")].join(" ")}>
                  {webAccessPrerequisiteLabel}
                </span>
              </div>
              <p className="mt-2 text-sm leading-6 text-[var(--color-text-secondary)]">
                {wm("prerequisites.webAccessStepDescription")}
              </p>
              <div className="mt-2 break-all font-mono text-xs text-[var(--color-text-primary)]">
                {configuredPublicUrl || wm("prerequisites.noPublicUrl")}
              </div>
              {onOpenWebAccess ? (
                <button type="button" onClick={onOpenWebAccess} className={[secondaryButtonClass("sm"), "mt-3"].join(" ")}>
                  {wm("buttons.openWebAccess")}
                </button>
              ) : null}
            </div>
            <div className="min-w-0 border-t border-[var(--glass-border-subtle)] pt-3 lg:border-t-0 lg:pt-0">
              <div className="flex flex-wrap items-center gap-2">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">{wm("prerequisites.actorStepTitle")}</div>
                <span className={["inline-flex rounded-full border px-2 py-0.5 text-xs font-semibold", setupPillClass(actorPrerequisiteTone)].join(" ")}>
                  {actorPrerequisiteLabel}
                </span>
              </div>
              <p className="mt-2 text-sm leading-6 text-[var(--color-text-secondary)]">
                {selectedActor ? wm("prerequisites.actorStepReadyDescription") : wm("prerequisites.actorStepDescription")}
              </p>
              <div className="mt-2 text-xs leading-5 text-[var(--color-text-tertiary)]">
                {chatGptActor ? (
                  <>
                    <span className="font-medium text-[var(--color-text-primary)]">{ownerGroupLabel || groupId}</span>
                    <span className="mx-1 text-[var(--color-text-muted)]">/</span>
                    <span className="font-mono text-[var(--color-text-primary)]">{ownerActorLabel || actorId}</span>
                  </>
                ) : groups.length ? (
                  <span>{wm("prerequisites.actorCreateHint")}</span>
                ) : (
                  <span>{wm("prerequisites.noGroupAvailable")}</span>
                )}
              </div>
              {extraChatGptActors.length ? (
                <div className="mt-3 rounded-lg border border-amber-500/25 bg-amber-500/10 px-3 py-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                  {wm("actorSection.multipleWarning")}
                </div>
              ) : null}
            </div>
          </div>
        </section>

        {!selectedActor ? (
          <section className={settingsWorkspacePanelClass(isDark)}>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{wm("empty.title")}</div>
            <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
              {wm("empty.description")}
            </p>
          </section>
        ) : null}

        {selectedActor ? (
          <section className={settingsWorkspacePanelClass(isDark)}>
            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div className="min-w-0">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {wm("chatSetup.title", { actor: selectedActorLabel || actorId })}
                </div>
                <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
                  {wm("chatSetup.description")}
                </p>
              </div>
              <div className="flex shrink-0 flex-wrap items-start gap-2 sm:justify-end">
                <span className={["inline-flex items-center rounded-full border px-3 py-1 text-xs font-semibold", setupPillClass(runtimeStatus.tone)].join(" ")}>
                  {runtimeStatus.label}
                </span>
              </div>
            </div>

            <div className="mt-4 space-y-4">
              <SetupSection title={wm("chatSetup.accountTitle")}>
                <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                  <div className="min-w-0 text-sm leading-6 text-[var(--color-text-secondary)]">
                    <span className="font-semibold text-[var(--color-text-primary)]">
                      {browserReady ? wm("browser.signedIn") : browserActive ? wm("browser.open") : wm("browser.notOpen")}
                    </span>
                    <span className="ml-2 text-xs text-[var(--color-text-tertiary)]">
                      {browserReady ? wm("chatSetup.accountReadyHint") : wm("chatSetup.accountOpenHint")}
                    </span>
                    {selectedBrowserSession?.error ? (
                      <div className="mt-1 text-xs leading-5 text-rose-600 dark:text-rose-300">{selectedBrowserSession.error}</div>
                    ) : null}
                  </div>
                  <div className="flex shrink-0 flex-wrap gap-2 md:justify-end">
                    <button
                      type="button"
                      onClick={() => void openBrowserLogin()}
                      disabled={browserBusy || !groupId || !actorId}
                      className={browserReady ? secondaryButtonClass("sm") : primaryButtonClass(browserBusy)}
                    >
                      {wm("buttons.openChatGpt")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void checkBrowserSessionStatus()}
                      disabled={browserBusy || !groupId || !actorId}
                      className={secondaryButtonClass("sm")}
                    >
                      {wm("buttons.checkStatus")}
                    </button>
                  </div>
                </div>
                {showBrowserSurface && groupId && actorId ? (
                  <div className="mt-3">
                    <div className="mb-2 flex flex-col gap-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2 sm:flex-row sm:items-center sm:justify-between">
                      <div className="text-xs leading-5 text-[var(--color-text-tertiary)]">
                        {wm("embedded.reloadDescription")}
                      </div>
                      <button
                        type="button"
                        onClick={() => void reloadEmbeddedBrowser()}
                        disabled={browserBusy || !groupId || !actorId}
                        className={secondaryButtonClass("sm")}
                      >
                        {wm("buttons.reloadChatGpt")}
                      </button>
                    </div>
                    <ProjectedBrowserSurfacePanel
                      key={`chatgpt-actor-surface:${groupId}:${actorId}:${browserSurfaceRestartNonce}`}
                      isDark={isDark}
                      refreshNonce={browserSurfaceRefreshNonce}
                      viewportClassName="h-[68vh] min-h-[460px] max-h-[780px]"
                      loadSession={loadBrowserSurfaceSession}
                      startSession={startBrowserSurfaceSession}
                      webSocketUrl={api.getWebModelBrowserSurfaceWebSocketUrl(groupId, actorId)}
                      fallbackUrl="https://chatgpt.com/"
                      labels={{
                        starting: wm("browserSurface.starting"),
                        waiting: wm("browserSurface.waiting"),
                        ready: wm("browserSurface.ready"),
                        failed: wm("browserSurface.failed"),
                        closed: wm("browserSurface.closed"),
                        reconnecting: wm("browserSurface.reconnecting"),
                        reconnect: wm("browserSurface.reconnect"),
                        frameAlt: wm("browserSurface.frameAlt"),
                      }}
                    />
                  </div>
                ) : null}
              </SetupSection>

              <SetupSection title={wm("chatSetup.mcpAppTitle")}>
                <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className={["inline-flex rounded-full border px-2 py-0.5 text-xs font-semibold", setupPillClass(mcpStatusTone)].join(" ")}>
                        {mcpStatusLabel}
                      </span>
                    </div>
                    <p className="mt-2 text-sm leading-6 text-[var(--color-text-secondary)]">
                      {mcpInstructionDetail}
                    </p>
                    <ol className="mt-3 list-decimal space-y-1 pl-4 text-xs leading-5 text-[var(--color-text-tertiary)]">
                      <li>{wm("mcp.instructionOpenSettings")}</li>
                      <li>{wm("mcp.instructionCreateApp")}</li>
                      <li>{wm("mcp.instructionEnableConnector")}</li>
                    </ol>
                    <div className="mt-3 rounded-md border border-amber-500/20 bg-amber-500/5 px-3 py-2 text-xs leading-5 text-amber-800 dark:text-amber-200">
                      <span>{wm("mcp.permissionHint")}</span>
                      <a
                        href="https://help.openai.com/en/articles/11487775-apps-in-chatgpt"
                        target="_blank"
                        rel="noreferrer"
                        className="ml-2 font-semibold underline-offset-2 hover:underline"
                      >
                        {wm("mcp.permissionDocsLink")}
                      </a>
                    </div>
                    {selectedConnector && !selectedMcpUrl ? (
                      <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                        {wm("warnings.rotateOldConnector")}
                      </div>
                    ) : null}
                    {mcpUrlLocalWarning ? (
                      <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                        {wm("warnings.localMcpUrl")}
                      </div>
                    ) : null}
                    {mcpUrlHttpsWarning && !mcpUrlLocalWarning ? (
                      <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                        {wm("warnings.nonHttpsMcpUrl")}
                      </div>
                    ) : null}
                  </div>
                  <div className="flex shrink-0 flex-wrap gap-2 md:justify-end">
                    {selectedMcpUrl ? (
                      <button
                        type="button"
                        onClick={() => void copyValue(selectedMcpUrl, wm("copyLabels.mcpUrl"))}
                        className={chatGptSeen ? secondaryButtonClass("sm") : primaryButtonClass(false)}
                      >
                        {wm("buttons.copyMcpUrl")}
                      </button>
                    ) : (
                      <button
                        type="button"
                        onClick={() => void createConnector(actorId)}
                        disabled={createBusy || !groupId || !actorId || !webAccessReady}
                        className={primaryButtonClass(createBusy)}
                      >
                        {selectedConnector ? wm("buttons.rotateMcpUrl") : wm("buttons.createMcpUrl")}
                      </button>
                    )}
                  </div>
                </div>
              </SetupSection>

              <SetupSection title={wm("chatSetup.deliveryTargetTitle")}>
                <div className="grid gap-3 xl:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]">
                  <div className="rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3">
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="text-xs font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                        {wm("target.savedTarget")}
                      </div>
                      <span className={["inline-flex rounded-full border px-2 py-0.5 text-xs font-semibold", setupPillClass(savedTargetTone)].join(" ")}>
                        {savedTargetLabel}
                      </span>
                    </div>
                    <div className="mt-2 break-all text-sm leading-6 text-[var(--color-text-secondary)]">
                      {savedTargetDetail}
                    </div>
                    <div className="mt-2 text-xs leading-5 text-[var(--color-text-tertiary)]">
                      {deliveryTargetSavedAt
                        ? wm("target.savedAt", { time: formatTime(deliveryTargetSavedAt) })
                        : wm("target.notSavedYet")}
                    </div>
                    <div className="mt-3 rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-2 text-xs leading-5 text-[var(--color-text-secondary)]">
                      <span className="font-semibold text-[var(--color-text-primary)]">{wm("target.nextDelivery")}</span>
                      <span className="ml-2">{nextDeliveryDetail}</span>
                    </div>
                  </div>

                  <div className="rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3">
                    <div className="text-xs font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                      {wm("target.currentBrowserTab")}
                    </div>
                    <div className="mt-2 break-all text-sm leading-6 text-[var(--color-text-secondary)]">
                      {currentBrowserDetail}
                    </div>
                    {boundConversationUrl && currentBrowserUrl && currentBrowserUrl !== boundConversationUrl ? (
                      <div className="mt-1 text-xs leading-5 text-[var(--color-text-tertiary)]">
                        {wm("target.currentTabNotTarget")}
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="mt-3 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 py-3">
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                      <div className="text-xs font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                        {wm("target.changeTarget")}
                      </div>
                      <div className="mt-1 text-sm leading-6 text-[var(--color-text-secondary)]">
                        {targetDraftDirty ? wm("target.unsavedChanges") : wm("target.noUnsavedChanges")}
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => void saveDeliveryTarget()}
                      disabled={targetSaveDisabled}
                      className={targetDraftDirty ? primaryButtonClass(browserBusy) : secondaryButtonClass("sm")}
                    >
                      {wm("buttons.saveTarget")}
                    </button>
                  </div>

                  <fieldset className="mt-3 space-y-3">
                    <legend className="sr-only">{wm("target.changeTarget")}</legend>
                    <label className={targetRadioClass("existing")}>
                      <input
                        type="radio"
                        name="chatgpt-delivery-target"
                        checked={targetDraftMode === "existing"}
                        onChange={() => chooseTargetMode("existing")}
                        className="mt-0.5 h-4 w-4 shrink-0 accent-[rgb(35,36,37)] dark:accent-white"
                      />
                      <span className="min-w-0">
                        <span className="block font-semibold">{wm("target.optionExisting")}</span>
                        <span className="mt-0.5 block text-xs leading-5 text-[var(--color-text-tertiary)]">
                          {wm("target.optionExistingDetail")}
                        </span>
                      </span>
                    </label>

                    {targetDraftMode === "existing" ? (
                      <div className="ml-6 space-y-2 border-l border-[var(--glass-border-subtle)] pl-3">
                        <label className="block">
                          <span className={labelClass(isDark)}>{wm("target.conversationUrl")}</span>
                          <div className="mt-1 flex flex-col gap-2 sm:flex-row">
                            <input
                              value={conversationUrlDraft}
                              onFocus={() => {
                                setTargetDraftMode("existing");
                                setTargetDraftTouched(true);
                              }}
                              onChange={(event) => {
                                setTargetDraftMode("existing");
                                setConversationUrlDraft(event.target.value);
                                setTargetDraftTouched(true);
                              }}
                              placeholder="https://chatgpt.com/c/..."
                              className={inputClass(isDark)}
                            />
                            {targetUseCurrentAvailable ? (
                              <button
                                type="button"
                                onClick={() => {
                                  setTargetDraftMode("existing");
                                  setConversationUrlDraft(currentBrowserConversationUrl);
                                  setTargetDraftTouched(true);
                                }}
                                className={secondaryButtonClass("sm")}
                              >
                                {wm("buttons.useCurrentTab")}
                              </button>
                            ) : null}
                          </div>
                        </label>
                        <div className="text-xs leading-5 text-[var(--color-text-tertiary)]">
                          {currentBrowserConversationUrl
                            ? wm("target.currentTab", { target: shortConversationLabel(currentBrowserConversationUrl) })
                            : wm("target.currentTabUnavailable")}
                        </div>
                      </div>
                    ) : null}

                    <label className={targetRadioClass("new")}>
                      <input
                        type="radio"
                        name="chatgpt-delivery-target"
                        checked={targetDraftMode === "new"}
                        onChange={() => chooseTargetMode("new")}
                        className="mt-0.5 h-4 w-4 shrink-0 accent-[rgb(35,36,37)] dark:accent-white"
                      />
                      <span className="min-w-0">
                        <span className="block font-semibold">{wm("target.optionNew")}</span>
                        <span className="mt-0.5 block text-xs leading-5 text-[var(--color-text-tertiary)]">
                          {wm("target.optionNewDetail")}
                        </span>
                      </span>
                    </label>
                  </fieldset>
                  {targetDraftError ? (
                    <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                      {targetDraftError}
                    </div>
                  ) : null}
                </div>
              </SetupSection>

              <details className="text-xs leading-5 text-[var(--color-text-tertiary)]">
                <summary className="cursor-pointer font-semibold text-[var(--color-text-secondary)]">{wm("advanced.summary")}</summary>
                <div className="mt-2 grid gap-x-4 gap-y-1 sm:grid-cols-[140px_1fr]">
                  <span>{wm("advanced.status")}</span>
                  <span>{runtimeStatus.label}</span>
                  <span>{wm("advanced.browser")}</span>
                  <span>{browserStatusLabel}</span>
                  <span>{wm("advanced.mcpApp")}</span>
                  <span>{mcpStatusLabel}</span>
                  {selectedConnector?.connector_id ? (
                    <>
                      <span>{wm("details.mcpUrlId")}</span>
                      <span className="break-all font-mono">{selectedConnector.connector_id}</span>
                    </>
                  ) : null}
                  {selectedConnector ? (
                    <>
                      <span>{wm("details.remote")}</span>
                      <span>{connectorActivityLabel(selectedConnector, wm)}</span>
                    </>
                  ) : null}
                  {selectedConnector?.last_error ? (
                    <>
                      <span>{wm("details.lastMcpError")}</span>
                      <span className="break-all text-rose-600 dark:text-rose-300">{selectedConnector.last_error}</span>
                    </>
                  ) : null}
                  <span>{wm("advanced.targetStatus")}</span>
                  <span>{targetStatusLabel}</span>
                  {healthNextActionText(selectedHealth, wm) ? (
                    <>
                      <span>{wm("advanced.recommended")}</span>
                      <span>{healthNextActionText(selectedHealth, wm)}</span>
                    </>
                  ) : null}
                  <span>{browserActive ? wm("advanced.currentBrowserTab") : wm("advanced.lastBrowserTab")}</span>
                  <span className="break-all font-mono">{currentBrowserUrl || wm("common.none")}</span>
                  <span>{wm("advanced.deliveryTarget")}</span>
                  <span className="break-all font-mono">{boundConversationUrl || (pendingNewChatBind ? wm("target.newChatNextDelivery") : wm("common.none"))}</span>
                  {selectedBrowserSession?.last_delivery_status || selectedBrowserSession?.last_delivery_at ? (
                    <>
                      <span>{wm("advanced.lastDelivery")}</span>
                      <span className="break-all font-mono">
                        {[selectedBrowserSession.last_delivery_status || wm("advanced.recorded"), selectedBrowserSession.last_submission_evidence || ""]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                    </>
                  ) : null}
                  {selectedBrowserSession?.last_error ? (
                    <>
                      <span>{wm("advanced.lastError")}</span>
                      <span className="break-all text-rose-600 dark:text-rose-300">{selectedBrowserSession.last_error}</span>
                    </>
                  ) : null}
                  {!boundConversationUrl && pendingNewChatBind ? (
                    <>
                      <span>{wm("advanced.pendingNewChat")}</span>
                      <span className="break-all font-mono">{pendingNewChatUrl || "https://chatgpt.com/"}</span>
                    </>
                  ) : null}
                  {selectedBrowserSession?.profile_dir ? (
                    <>
                      <span>{wm("details.profile")}</span>
                      <span className="break-all font-mono">{selectedBrowserSession.profile_dir}</span>
                    </>
                  ) : null}
                  {selectedBrowserSession?.visibility ? (
                    <>
                      <span>{wm("advanced.mode")}</span>
                      <span>{selectedBrowserSession.visibility}</span>
                    </>
                  ) : null}
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  {selectedConnector ? (
                    <button
                      type="button"
                      onClick={() => void createConnector(actorId)}
                      disabled={createBusy || !groupId || !actorId}
                      className={secondaryButtonClass("sm")}
                    >
                      {wm("buttons.rotateMcpUrl")}
                    </button>
                  ) : null}
                  {selectedConnector ? (
                    <button
                      type="button"
                      onClick={() => void revokeConnector(selectedConnector.connector_id)}
                      disabled={revokeBusyId === selectedConnector.connector_id}
                      className={dangerButtonClass("sm")}
                    >
                      {wm("buttons.revokeMcpUrl")}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => void checkBrowserSessionStatus()}
                    disabled={browserBusy || !groupId || !actorId}
                    className={secondaryButtonClass("sm")}
                  >
                    {wm("buttons.checkStatus")}
                  </button>
                  <button
                    type="button"
                    onClick={() => void closeBrowserSession()}
                    disabled={browserBusy || !browserActive}
                    className={secondaryButtonClass("sm")}
                  >
                    {wm("buttons.closeBrowser")}
                  </button>
                </div>
              </details>
            </div>
          </section>
        ) : null}

      </div>
    </div>
  );
}
