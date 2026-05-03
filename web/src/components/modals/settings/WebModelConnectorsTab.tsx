import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Actor, GroupMeta, RemoteAccessState } from "../../../types";
import * as api from "../../../services/api";
import { copyTextToClipboard } from "../../../utils/copy";
import { ProjectedBrowserSurfacePanel } from "../../browser/ProjectedBrowserSurfacePanel";
import { SelectCombobox } from "../../SelectCombobox";
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

function connectorActivityLabel(connector: api.WebModelConnector): string {
  const status = String(connector.last_call_status || "").trim();
  const wait = String(connector.last_wait_status || "").trim();
  const tool = String(connector.last_tool_name || "").trim();
  if (!connector.last_activity_at) return "not seen yet";
  if (status === "error") return "last call failed";
  if (tool === "cccc_runtime_wait_next_turn" && wait) return `wait: ${wait}`;
  if (tool === "cccc_runtime_complete_turn" && wait) return `complete: ${wait}`;
  return tool || String(connector.last_method || "").trim() || "seen";
}

function webModelQueuedCount(actor?: Actor | null): number {
  return Math.max(0, Number(actor?.web_model_queued_count || 0));
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

function isChatGptConversationUrl(url?: string): boolean {
  const value = String(url || "").trim();
  if (!value) return false;
  try {
    const parsed = new URL(value);
    if (parsed.protocol !== "https:") return false;
    const host = parsed.hostname.toLowerCase();
    if (host !== "chatgpt.com" && !host.endsWith(".chatgpt.com")) return false;
    const parts = parsed.pathname.split("/").filter(Boolean);
    return parts.some((part, index) => part === "c" && Boolean(parts[index + 1]));
  } catch {
    return false;
  }
}

function isChatGptUrl(url?: string): boolean {
  const value = String(url || "").trim();
  if (!value) return false;
  try {
    const parsed = new URL(value);
    if (parsed.protocol !== "https:") return false;
    const host = parsed.hostname.toLowerCase();
    return host === "chatgpt.com" || host.endsWith(".chatgpt.com");
  } catch {
    return false;
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

export default function WebModelConnectorsTab({
  isDark,
  isActive = true,
  currentGroupId = "",
  onOpenWebAccess,
}: WebModelConnectorsTabProps) {
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [actors, setActors] = useState<Actor[]>([]);
  const [connectors, setConnectors] = useState<api.WebModelConnector[]>([]);
  const [remoteState, setRemoteState] = useState<RemoteAccessState | null>(null);
  const [groupId, setGroupId] = useState("");
  const [actorId, setActorId] = useState("");
  const [busy, setBusy] = useState(false);
  const [createBusy, setCreateBusy] = useState(false);
  const [startBusyId, setStartBusyId] = useState("");
  const [revokeBusyId, setRevokeBusyId] = useState("");
  const [actorCreateBusy, setActorCreateBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [browserSession, setBrowserSession] = useState<api.WebModelBrowserSession | null>(null);
  const [browserSessionsByActor, setBrowserSessionsByActor] = useState<Record<string, api.WebModelBrowserSession>>({});
  const [browserBusy, setBrowserBusy] = useState(false);
  const [showBrowserSurface, setShowBrowserSurface] = useState(false);
  const [chatManagerOpen, setChatManagerOpen] = useState(false);
  const [browserSurfaceRefreshNonce, setBrowserSurfaceRefreshNonce] = useState(0);
  const [browserSurfaceRestartNonce, setBrowserSurfaceRestartNonce] = useState(0);
  const [conversationUrlDraft, setConversationUrlDraft] = useState("");
  const [createActorGroupId, setCreateActorGroupId] = useState("");
  const currentSelectionRef = useRef({ groupId: "", actorId: "" });

  useEffect(() => {
    currentSelectionRef.current = { groupId, actorId };
  }, [actorId, groupId]);

  const webModelActors = useMemo(
    () => actors.filter((actor) => String(actor.runtime || "").trim().toLowerCase() === "web_model"),
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
  const actorRows = useMemo(
    () => webModelActors.map((actor) => ({
      actor,
      connector: currentGroupActiveConnectors.find((connector) => String(connector.actor_id || "") === actor.id) || null,
    })),
    [currentGroupActiveConnectors, webModelActors],
  );
  const chatGptActorRow = actorRows[0] || null;
  const extraChatGptActorRows = actorRows.slice(1);
  const ownerGroup = useMemo(
    () => groups.find((group) => String(group.group_id || "").trim() === groupId) || null,
    [groupId, groups],
  );
  const ownerGroupLabel = String(ownerGroup?.title || ownerGroup?.group_id || "").trim();
  const ownerActorLabel = chatGptActorRow
    ? String(chatGptActorRow.actor.title || chatGptActorRow.actor.id || "").trim()
    : "";
  const preferredCreateActorGroupId = useMemo(() => {
    const preferred = String(currentGroupId || "").trim();
    if (preferred && groups.some((group) => String(group.group_id || "").trim() === preferred)) return preferred;
    return String(groups[0]?.group_id || "").trim();
  }, [currentGroupId, groups]);
  const createActorGroup = useMemo(
    () => groups.find((group) => String(group.group_id || "").trim() === createActorGroupId) || null,
    [createActorGroupId, groups],
  );
  const createActorGroupLabel = String(createActorGroup?.title || createActorGroup?.group_id || "").trim();

  const configuredPublicUrl = String(remoteState?.config?.web_public_url || remoteState?.diagnostics?.web_public_url || "").trim();
  const publicEndpointReady = Boolean(configuredPublicUrl && isHttpsUrl(configuredPublicUrl));
  const uiAccessTokenPresent = Boolean(remoteState?.config?.access_token_configured || remoteState?.diagnostics?.access_token_present);
  const selectedBrowserSession = browserSessionsByActor[browserSessionKey(groupId, actorId)] || browserSession || null;
  const browserActive = Boolean(selectedBrowserSession?.active || showBrowserSurface);
  const browserReady = Boolean(selectedBrowserSession?.ready);
  const boundConversationUrl = String(selectedBrowserSession?.conversation_url || "").trim();
  const pendingNewChatBind = Boolean(selectedBrowserSession?.pending_new_chat_bind);
  const pendingNewChatUrl = String(selectedBrowserSession?.pending_new_chat_url || "").trim();
  const currentBrowserUrl = String(selectedBrowserSession?.tab_url || selectedBrowserSession?.last_tab_url || "").trim();
  const currentBrowserConversationUrl = isChatGptConversationUrl(currentBrowserUrl) ? currentBrowserUrl : "";
  const hasDeliveryTarget = Boolean(boundConversationUrl || pendingNewChatBind);
  const targetModeLabel = boundConversationUrl
    ? "Existing ChatGPT chat"
    : pendingNewChatBind
      ? "New chat on next delivery"
      : "No target selected";
  const targetModeTone: SetupTone = hasDeliveryTarget ? "ready" : "needs";
  const targetModeDetail = boundConversationUrl
    ? shortConversationLabel(boundConversationUrl)
    : pendingNewChatBind
      ? "CCCC will create and bind the /c/... URL after the next message is sent."
      : "Choose an existing chat or select new-chat delivery before sending work.";
  const browserStatusLabel = browserReady
    ? "ChatGPT ready"
    : browserActive
      ? "Sign-in needed"
      : "Not open";
  const targetStatusLabel = boundConversationUrl
    ? "Target chat bound"
    : pendingNewChatBind
      ? "New chat armed"
      : "Target chat needed";
  const setupReady = browserReady && Boolean(boundConversationUrl || pendingNewChatBind);
  const setupSummary = setupReady
    ? `${browserStatusLabel} · ${targetStatusLabel}`
    : !browserReady
      ? "Sign in to ChatGPT, then choose where this actor should deliver messages."
      : "Choose the ChatGPT conversation where this actor should deliver messages.";
  const selectedActorLabel = selectedActor ? String(selectedActor.title || selectedActor.id || "").trim() : "";

  const pushNotice = useCallback((value: string) => {
    setNotice(value);
    window.setTimeout(() => setNotice(""), 1600);
  }, []);

  const loadConnectors = useCallback(async () => {
    const resp = await api.fetchWebModelConnectors();
    if (resp.ok) {
      setConnectors(resp.result?.connectors || []);
    } else {
      setError(resp.error?.message || "Failed to load ChatGPT Web Model MCP URL.");
    }
  }, []);

  const loadBrowserSession = useCallback(async (gid: string = groupId, aid: string = actorId) => {
    if ((gid && !aid) || (!gid && aid)) {
      setBrowserSession(null);
      return;
    }
    const resp = await api.fetchWebModelBrowserSession(gid, aid);
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
      setError(resp.error?.message || "Failed to load ChatGPT browser session.");
    }
  }, [actorId, groupId]);

  const loadBrowserSessionsForActors = useCallback(async (gid: string, rows: Actor[]) => {
    if (!gid || !rows.length) {
      setBrowserSessionsByActor({});
      return;
    }
    const entries = await Promise.all(
      rows.map(async (actor) => {
        const aid = String(actor.id || "").trim();
        if (!aid) return null;
        const resp = await api.fetchWebModelBrowserSession(gid, aid);
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
        if (current && nextActors.some((actor) => actor.id === current && actor.runtime === "web_model")) return current;
        return nextActors.find((actor) => actor.runtime === "web_model")?.id || "";
      });
    } else {
      setActors([]);
      setActorId("");
      setError(resp.error?.message || "Failed to load actors.");
    }
  }, []);

  const loadBrowserSurfaceSession = useCallback(async () => {
    const gid = groupId;
    const aid = actorId;
    const resp = await api.fetchWebModelBrowserSurfaceSession(gid, aid);
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
      setError(resp.error?.message || "Failed to load ChatGPT browser session.");
    }
    return resp;
  }, [actorId, groupId]);

  const startBrowserSurfaceSession = useCallback(async (size: { width: number; height: number }) => {
    const gid = groupId;
    const aid = actorId;
    const resp = await api.openWebModelBrowserSurfaceSession({
      groupId: gid,
      actorId: aid,
      width: size.width,
      height: size.height,
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
      setError(resp.error?.message || "Failed to open ChatGPT browser.");
    }
    return resp;
  }, [actorId, groupId]);

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
        setCreateActorGroupId((current) => {
          if (current && nextGroups.some((group) => String(group.group_id || "").trim() === current)) return current;
          const preferred = String(currentGroupId || "").trim();
          if (preferred && nextGroups.some((group) => String(group.group_id || "").trim() === preferred)) return preferred;
          return String(nextGroups[0]?.group_id || "").trim();
        });
      } else {
        setError(groupsResp.error?.message || "Failed to load groups.");
      }
    } catch {
      setError("Failed to load ChatGPT Web Model settings.");
    } finally {
      setBusy(false);
    }
  }, [currentGroupId, isActive, loadConnectors]);

  useEffect(() => {
    void loadInitial();
  }, [loadInitial]);

  useEffect(() => {
    if (!isActive || !groups.length) return;
    let cancelled = false;
    const locateExistingActor = async () => {
      for (const group of groups) {
        const gid = String(group.group_id || "").trim();
        if (!gid) continue;
        const resp = await api.fetchActors(gid, true, { noCache: true });
        if (cancelled) return;
        if (!resp.ok) continue;
        const found = (resp.result?.actors || []).find(
          (actor) => String(actor.runtime || "").trim().toLowerCase() === "web_model",
        );
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
      setChatManagerOpen(false);
      setShowBrowserSurface(false);
    };
    void locateExistingActor();
    return () => {
      cancelled = true;
    };
  }, [groups, isActive]);

  useEffect(() => {
    if (!isActive || !groupId) {
      setActors([]);
      setActorId("");
      setBrowserSession(null);
      setBrowserSessionsByActor({});
      setChatManagerOpen(false);
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
    if (!isActive || !groupId || !actorId || !selectedActor || (!chatManagerOpen && !showBrowserSurface)) return;
    let cancelled = false;
    const refresh = async () => {
      const gid = groupId;
      const aid = actorId;
      const resp = await api.fetchWebModelBrowserSession(gid, aid);
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
  }, [actorId, chatManagerOpen, groupId, isActive, selectedActor, showBrowserSurface]);

  const createChatGptWebModelActor = async () => {
    const gid = String(createActorGroupId || preferredCreateActorGroupId || "").trim();
    if (!gid) {
      setError("Create or select a group before adding ChatGPT Web Model.");
      return;
    }
    setActorCreateBusy(true);
    setError("");
    try {
      const resp = await api.addActor(
        gid,
        "",
        "peer",
        "web_model",
        "headless",
        "",
        undefined,
        { title: "ChatGPT Web Model" },
      );
      if (!resp.ok) {
        setError(resp.error?.message || "Failed to create ChatGPT Web Model actor.");
        return;
      }
      const createdActorId = String(resp.result?.actor?.id || "").trim();
      setGroupId(gid);
      if (createdActorId) setActorId(createdActorId);
      await loadActorsForGroup(gid);
      if (createdActorId) {
        setActorId(createdActorId);
        await loadBrowserSession(gid, createdActorId);
      }
      setChatManagerOpen(true);
      setShowBrowserSurface(true);
      setBrowserSurfaceRefreshNonce((value) => value + 1);
      pushNotice("ChatGPT Web Model actor created.");
    } catch {
      setError("Failed to create ChatGPT Web Model actor.");
    } finally {
      setActorCreateBusy(false);
    }
  };

  const createConnector = async (targetActorId = actorId) => {
    const aid = String(targetActorId || "").trim();
    if (!groupId || !aid) {
      setError("Select a group with the ChatGPT Web Model actor first.");
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
        pushNotice(replaced.length ? "Connector rotated. Previous connector revoked." : "Connector created.");
        await loadConnectors();
      } else {
        setError(resp.error?.message || "Failed to create connector.");
      }
    } catch {
      setError("Failed to create connector.");
    } finally {
      setCreateBusy(false);
    }
  };

  const startWebModelActor = async (targetActorId: string) => {
    const aid = String(targetActorId || "").trim();
    if (!groupId || !aid) return;
    setStartBusyId(aid);
    setError("");
    try {
      const resp = await api.startActor(groupId, aid);
      if (resp.ok) {
        pushNotice("Actor started.");
        await loadActorsForGroup(groupId);
        setChatManagerOpen(true);
        setShowBrowserSurface(true);
        setBrowserSurfaceRefreshNonce((value) => value + 1);
      } else {
        setError(resp.error?.message || "Failed to start actor.");
      }
    } catch {
      setError("Failed to start actor.");
    } finally {
      setStartBusyId("");
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
        setError(resp.error?.message || "Failed to revoke connector.");
      }
    } catch {
      setError("Failed to revoke connector.");
    } finally {
      setRevokeBusyId("");
    }
  };

  const openBrowserLogin = async () => {
    setError("");
    setChatManagerOpen(true);
    setShowBrowserSurface(true);
    setBrowserSurfaceRefreshNonce((value) => value + 1);
    pushNotice("ChatGPT sign-in surface opened below.");
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
        pushNotice("ChatGPT browser restarted.");
      } else {
        setError(resp.error?.message || "Failed to restart ChatGPT browser.");
      }
    } catch {
      setError("Failed to restart ChatGPT browser.");
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
          pushNotice("ChatGPT browser session closed.");
        }
      } else {
        setError(resp.error?.message || "Failed to close ChatGPT browser session.");
      }
    } catch {
      setError("Failed to close ChatGPT browser session.");
    } finally {
      setBrowserBusy(false);
    }
  };

  const bindConversation = async (conversationUrl = "", options?: { newChat?: boolean }) => {
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
          setConversationUrlDraft("");
          pushNotice(options?.newChat ? "New ChatGPT chat selected for next delivery." : "ChatGPT conversation bound to this actor.");
        }
      } else {
        setError(resp.error?.message || "Failed to bind ChatGPT conversation.");
      }
    } catch {
      setError("Failed to bind ChatGPT conversation.");
    } finally {
      setBrowserBusy(false);
    }
  };

  const startNewConversationAutoBind = async () => {
    const gid = groupId;
    const aid = actorId;
    const seedUrl = isChatGptUrl(currentBrowserUrl) && !isChatGptConversationUrl(currentBrowserUrl)
      ? currentBrowserUrl
      : "https://chatgpt.com/";
    await bindConversation(seedUrl, { newChat: true });
    const currentSelection = currentSelectionRef.current;
    if (gid === currentSelection.groupId && aid === currentSelection.actorId) {
      setShowBrowserSurface(true);
      setBrowserSurfaceRefreshNonce((value) => value + 1);
    }
  };

  const manageActorChat = (aid: string) => {
    const nextActorId = String(aid || "").trim();
    if (!nextActorId) return;
    const key = browserSessionKey(groupId, nextActorId);
    setActorId(nextActorId);
    setBrowserSession(browserSessionsByActor[key] || null);
    setChatManagerOpen(true);
    setShowBrowserSurface(false);
    setConversationUrlDraft(String(browserSessionsByActor[key]?.conversation_url || ""));
    void loadBrowserSession(groupId, nextActorId);
  };

  const copyValue = async (value: string, labelText: string) => {
    const ok = await copyTextToClipboard(value);
    pushNotice(ok ? `${labelText} copied.` : "Copy failed.");
  };

  return (
    <div className={settingsWorkspaceShellClass(isDark)}>
      <div className={settingsWorkspaceHeaderClass(isDark)}>
        <div className="min-w-0">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--color-text-muted)]">
            ChatGPT Runtime
          </div>
          <h3 className="mt-1 text-base font-semibold text-[var(--color-text-primary)]">ChatGPT Web Model setup</h3>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-[var(--color-text-tertiary)]">
            Connect one ChatGPT conversation to CCCC as a real actor: public CCCC endpoint, one ChatGPT MCP URL,
            ChatGPT sign-in, then one target conversation.
          </p>
        </div>
        <button type="button" onClick={() => void loadInitial()} disabled={busy} className={secondaryButtonClass("sm")}>
          Refresh
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
          <div className="grid gap-4 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)_auto] lg:items-start">
            <div className="min-w-0">
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">Current owner</div>
              <div className="mt-1 text-sm leading-6 text-[var(--color-text-secondary)]">
                {chatGptActorRow ? (
                  <>
                    <span className="font-medium text-[var(--color-text-primary)]">{ownerGroupLabel || groupId}</span>
                    <span className="mx-1 text-[var(--color-text-muted)]">/</span>
                    <span className="font-mono text-[var(--color-text-primary)]">{ownerActorLabel || actorId}</span>
                  </>
                ) : (
                  <span>No ChatGPT Web Model actor configured</span>
                )}
              </div>
              <div className="mt-1 text-xs leading-5 text-[var(--color-text-tertiary)]">
                ChatGPT Web Model is a single CCCC runtime seat. It belongs to one group/actor identity and is configured here globally.
              </div>
            </div>
            <div className="min-w-0">
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">Global prerequisites</div>
              <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
                ChatGPT connects from the cloud, so CCCC needs a public HTTPS Web URL. The copied MCP URL already contains
                the actor token; set ChatGPT Authentication to No Auth.
              </p>
              <div className="mt-2 break-all font-mono text-xs text-[var(--color-text-primary)]">
                {configuredPublicUrl || "No public Web URL configured"}
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <span
                  className={[
                    "inline-flex items-center rounded-full border px-3 py-1 text-xs font-semibold",
                    publicEndpointReady
                      ? "border-emerald-500/30 bg-emerald-500/12 text-emerald-700 dark:text-emerald-300"
                      : "border-amber-500/30 bg-amber-500/12 text-amber-700 dark:text-amber-300",
                  ].join(" ")}
                >
                  {publicEndpointReady ? "Public HTTPS ready" : "Public HTTPS needed"}
                </span>
                <span
                  className={[
                    "inline-flex items-center rounded-full border px-3 py-1 text-xs font-semibold",
                    uiAccessTokenPresent
                      ? "border-emerald-500/30 bg-emerald-500/12 text-emerald-700 dark:text-emerald-300"
                      : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]",
                  ].join(" ")}
                >
                  {uiAccessTokenPresent ? "Access token ready" : "Access token needed"}
                </span>
              </div>
            </div>
            {onOpenWebAccess ? (
              <button type="button" onClick={onOpenWebAccess} className={secondaryButtonClass("sm")}>
                Open Web Access
              </button>
            ) : null}
          </div>
        </section>

        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">ChatGPT Web Model actor</div>
              <div className="mt-1 text-xs text-[var(--color-text-tertiary)]">
                One ChatGPT MCP app maps to one CCCC actor. Create another CCCC actor only after removing this one.
              </div>
            </div>
          </div>
          {extraChatGptActorRows.length ? (
            <div className="mt-3 rounded-lg border border-amber-500/25 bg-amber-500/10 px-3 py-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
              Multiple ChatGPT Web Model actors exist in this group. This setup supports one; keep one actor and remove the extras.
            </div>
          ) : null}
          <div className="mt-3 space-y-3">
            {chatGptActorRow ? [chatGptActorRow].map(({ actor, connector }) => {
              const queuedCount = webModelQueuedCount(actor);
              const mcpUrl = connectorMcpUrl(connector || null);
              const connectorUrl = String(connector?.connector_url || "").trim();
              const rowLocalWarning = Boolean(mcpUrl || connectorUrl) && isLocalConnectorUrl(mcpUrl || connectorUrl);
              const rowHttpsWarning = Boolean(mcpUrl || connectorUrl) && !isHttpsUrl(mcpUrl || connectorUrl);
              const actorLabel = String(actor.title || actor.id || "").trim() || actor.id;
              const selected = actor.id === actorId;
              const rowSession = browserSessionsByActor[browserSessionKey(groupId, actor.id)] || {};
              const targetUrl = String(rowSession.conversation_url || "").trim();
              const rowPendingNewChat = Boolean(rowSession.pending_new_chat_bind);
              const rowPendingUrl = String(rowSession.pending_new_chat_url || "").trim();
              const targetReady = Boolean(targetUrl);
              const actorRunning = Boolean(actor.running);
              const chatGptSeen = Boolean(connector?.last_activity_at);
              const browserLoginReady = Boolean(rowSession.ready);
              const browserOpen = Boolean(rowSession.active);
              const targetDetail = targetReady
                ? shortConversationLabel(targetUrl)
                : rowPendingNewChat
                  ? "New chat armed"
                  : "Not set";
              const mcpDetail = mcpUrl ? (chatGptSeen ? "Connected" : "URL ready") : connector ? "Needs rotation" : "Not created";
              const browserDetail = browserLoginReady ? "Signed in" : browserOpen ? "Open, sign-in needed" : "Not open";
              const cardStatus = !publicEndpointReady
                ? { label: "Needs public URL", tone: "needs" as const }
                : !actorRunning
                  ? { label: "Needs start", tone: "needs" as const }
                  : !mcpUrl
                    ? { label: "Needs MCP URL", tone: "needs" as const }
                    : !chatGptSeen
                      ? { label: "Connect in ChatGPT", tone: "needs" as const }
                      : !browserLoginReady
                        ? { label: "Needs sign-in", tone: "needs" as const }
                        : !targetReady && !rowPendingNewChat
                          ? { label: "Needs target chat", tone: "needs" as const }
                          : { label: "Ready", tone: "ready" as const };
              const mcpTone: SetupTone = mcpUrl && chatGptSeen ? "ready" : mcpUrl ? "neutral" : "needs";
              const targetTone: SetupTone = targetReady || rowPendingNewChat ? "ready" : "needs";
              const browserTone: SetupTone = browserLoginReady ? "ready" : browserOpen ? "needs" : "neutral";
              const nextAction = !publicEndpointReady
                ? "Set the public HTTPS URL in Web Access."
                : !actorRunning
                  ? "Start the ChatGPT Web Model actor."
                  : !mcpUrl
                    ? connector
                      ? "Rotate the connector once to generate the ChatGPT MCP URL."
                      : "Create the ChatGPT MCP URL."
                    : !chatGptSeen
                      ? "Paste the MCP URL into ChatGPT New App with Authentication set to No Auth."
                    : !targetReady && !rowPendingNewChat
                      ? "Bind an existing ChatGPT conversation or arm new-chat auto-bind."
                    : !browserLoginReady
                      ? "Open the embedded browser and sign in to ChatGPT."
                      : "Ready for browser delivery.";
              const setupPrimary = actorRunning && Boolean(mcpUrl) && chatGptSeen && (!browserLoginReady || (!targetReady && !rowPendingNewChat));
              return (
                <div
                  key={actor.id}
                  className={[
                    "rounded-xl border px-3 py-3 sm:px-4",
                    selected
                      ? "border-[rgba(35,36,37,0.28)] bg-[var(--glass-tab-bg)] dark:border-white/18"
                      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]",
                  ].join(" ")}
                >
                  <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2 text-sm font-semibold text-[var(--color-text-primary)]">
                        <span>{actorLabel}</span>
                        <span
                          className={[
                            "rounded-full border px-2 py-0.5 text-[11px] font-medium",
                            setupPillClass(cardStatus.tone),
                          ].join(" ")}
                        >
                          {cardStatus.label}
                        </span>
                        {queuedCount > 0 ? (
                          <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2 py-0.5 text-[11px] font-medium text-amber-700 dark:text-amber-200">
                            {queuedCount} queued
                          </span>
                        ) : null}
                      </div>

                      <div className="mt-3 grid gap-3 sm:grid-cols-3">
                        <SetupStatusLine label="MCP app" detail={mcpDetail} tone={mcpTone} />
                        <SetupStatusLine label="ChatGPT session" detail={browserDetail} tone={browserTone} />
                        <SetupStatusLine label="Target chat" detail={targetDetail} tone={targetTone} />
                      </div>

                      <div className="mt-3 text-sm leading-5 text-[var(--color-text-secondary)]">
                        Next: {nextAction}
                      </div>

                      {connector && !mcpUrl ? (
                        <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                          Rotate this older connector once to generate a copyable ChatGPT MCP URL.
                        </div>
                      ) : null}
                      {rowLocalWarning ? (
                        <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                          MCP URL is local-only. Browser-hosted models need a public HTTPS URL.
                        </div>
                      ) : null}
                      {rowHttpsWarning && !rowLocalWarning ? (
                        <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                          MCP URL is not HTTPS. Use a public HTTPS tunnel or reverse proxy.
                        </div>
                      ) : null}

                      <details className="mt-3 text-xs leading-5 text-[var(--color-text-tertiary)]">
                        <summary className="cursor-pointer font-semibold text-[var(--color-text-secondary)]">Details and controls</summary>
                        <div className="mt-2 grid gap-x-3 gap-y-1 sm:grid-cols-[120px_1fr]">
                          {connector ? (
                            <>
                              <span>ChatGPT app</span>
                              <span>{chatGptSeen ? `seen ${formatTime(connector.last_activity_at)}` : "not seen yet"}</span>
                            </>
                          ) : (
                            <>
                              <span>ChatGPT app</span>
                              <span>No MCP app URL yet</span>
                            </>
                          )}
                          <span>Target chat</span>
                          <span className="break-all font-mono">
                            {targetUrl || (rowPendingNewChat ? `new pending ${rowPendingUrl || "https://chatgpt.com/"}` : "none")}
                          </span>
                          {connector?.connector_id ? (
                            <>
                              <span>MCP URL id</span>
                              <span className="break-all font-mono">{connector.connector_id}</span>
                            </>
                          ) : null}
                          {connector ? (
                            <>
                              <span>Remote</span>
                              <span>{connectorActivityLabel(connector)}</span>
                            </>
                          ) : null}
                          {connector?.last_error ? (
                            <>
                              <span>Last MCP error</span>
                              <span className="break-all text-rose-600 dark:text-rose-300">{connector.last_error}</span>
                            </>
                          ) : null}
                          {rowSession.last_turn_id ? (
                            <>
                              <span>Last turn</span>
                              <span className="break-all font-mono">{rowSession.last_turn_id}</span>
                            </>
                          ) : null}
                          {rowSession.profile_dir ? (
                            <>
                              <span>Profile</span>
                              <span className="break-all font-mono">{rowSession.profile_dir}</span>
                            </>
                          ) : null}
                        </div>
                        <div className="mt-3 flex flex-wrap gap-2">
                          {connector ? (
                            <button
                              type="button"
                              onClick={() => void createConnector(actor.id)}
                              disabled={createBusy || !groupId}
                              className={secondaryButtonClass("sm")}
                            >
                              Rotate MCP URL
                            </button>
                          ) : null}
                          {connector ? (
                            <button
                              type="button"
                              onClick={() => void revokeConnector(connector.connector_id)}
                              disabled={revokeBusyId === connector.connector_id}
                              className={dangerButtonClass("sm")}
                            >
                              Revoke MCP URL
                            </button>
                          ) : null}
                        </div>
                      </details>
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-2 xl:max-w-[360px] xl:justify-end">
                      {!actorRunning ? (
                        <button
                          type="button"
                          onClick={() => void startWebModelActor(actor.id)}
                          disabled={startBusyId === actor.id}
                          className={primaryButtonClass(startBusyId === actor.id)}
                        >
                          Start actor
                        </button>
                      ) : null}
                      {mcpUrl ? (
                        <button
                          type="button"
                          onClick={() => void copyValue(mcpUrl, "MCP URL")}
                          className={chatGptSeen ? secondaryButtonClass("sm") : primaryButtonClass(false)}
                        >
                          Copy MCP URL
                        </button>
                      ) : (
                        <button
                          type="button"
                          onClick={() => void createConnector(actor.id)}
                          disabled={createBusy || !groupId}
                          className={primaryButtonClass(createBusy)}
                        >
                          Create MCP URL
                        </button>
                      )}
                      <button
                        type="button"
                        onClick={() => manageActorChat(actor.id)}
                        className={setupPrimary ? primaryButtonClass(false) : secondaryButtonClass("sm")}
                      >
                        Set up ChatGPT
                      </button>
                    </div>
                  </div>
                </div>
              );
            }) : (
              <div className="rounded-lg border border-dashed border-[var(--glass-border-subtle)] px-3 py-4">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  Create the ChatGPT Web Model actor here
                </div>
                <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
                  This creates the single CCCC actor identity that the ChatGPT MCP app will use. After creation, continue here to create the MCP URL, sign in to ChatGPT, and choose the target chat.
                </p>
                <div className="mt-3 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3">
                  <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                    <div className="min-w-0">
                      <div className="text-sm font-semibold text-[var(--color-text-primary)]">ChatGPT browser</div>
                      <p className="mt-1 text-xs leading-5 text-[var(--color-text-tertiary)]">
                        You can sign in before creating the actor. CCCC keeps this one global ChatGPT page and reuses it later.
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => void openBrowserLogin()}
                      disabled={browserBusy}
                      className={primaryButtonClass(browserBusy)}
                    >
                      Open ChatGPT
                    </button>
                  </div>
                  {showBrowserSurface ? (
                    <div className="mt-3 overflow-hidden rounded-xl border border-[var(--glass-border-subtle)]">
                      <ProjectedBrowserSurfacePanel
                        key={`chatgpt-setup-surface:${browserSurfaceRestartNonce}`}
                        isDark={isDark}
                        refreshNonce={browserSurfaceRefreshNonce}
                        viewportClassName="h-[58vh] min-h-[420px] max-h-[720px]"
                        loadSession={loadBrowserSurfaceSession}
                        startSession={startBrowserSurfaceSession}
                        webSocketUrl={api.getWebModelBrowserSurfaceWebSocketUrl("", "")}
                        fallbackUrl="https://chatgpt.com/"
                        labels={{
                          starting: "Opening ChatGPT...",
                          waiting: "Waiting for ChatGPT...",
                          ready: "ChatGPT surface ready",
                          failed: "ChatGPT surface failed",
                          closed: "ChatGPT surface closed.",
                          reconnecting: "Reconnecting ChatGPT surface...",
                          reconnect: "Reconnect",
                          frameAlt: "ChatGPT browser frame",
                        }}
                      />
                    </div>
                  ) : null}
                </div>
                <div className="mt-3 grid gap-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
                  <label className="block">
                    <span className={labelClass(isDark)}>Owner group</span>
                    {groups.length > 1 ? (
                      <SelectCombobox
                        items={groups
                          .map((group) => {
                            const gid = String(group.group_id || "").trim();
                            const label = String(group.title || group.group_id || "").trim();
                            return gid ? { value: gid, label } : null;
                          })
                          .filter((item): item is { value: string; label: string } => item !== null)}
                        value={createActorGroupId}
                        onChange={setCreateActorGroupId}
                        ariaLabel="Owner group"
                        className={inputClass(isDark)}
                        searchable
                      />
                    ) : (
                      <div className="rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2 text-sm text-[var(--color-text-secondary)]">
                        {createActorGroupLabel || "No group available"}
                      </div>
                    )}
                  </label>
                  <button
                    type="button"
                    onClick={() => void createChatGptWebModelActor()}
                    disabled={actorCreateBusy || !String(createActorGroupId || preferredCreateActorGroupId || "").trim()}
                    className={primaryButtonClass(actorCreateBusy)}
                  >
                    Create actor
                  </button>
                </div>
                {!groups.length ? (
                  <div className="mt-2 text-xs leading-5 text-amber-700 dark:text-amber-300">
                    Create a CCCC group first, then return here to add ChatGPT Web Model.
                  </div>
                ) : null}
              </div>
            )}
          </div>
        </section>

        {chatManagerOpen && selectedActor ? (
          <section className={settingsWorkspacePanelClass(isDark)}>
            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div className="min-w-0">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  ChatGPT setup for {selectedActorLabel || actorId}
                </div>
                <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
                  {setupSummary}
                </p>
              </div>
              <div className="flex shrink-0 flex-wrap items-start gap-2 sm:justify-end">
                <span
                  className={[
                    "inline-flex items-center rounded-full border px-3 py-1 text-xs font-semibold",
                    setupReady
                      ? "border-emerald-500/30 bg-emerald-500/12 text-emerald-700 dark:text-emerald-300"
                      : "border-amber-500/30 bg-amber-500/12 text-amber-700 dark:text-amber-300",
                  ].join(" ")}
                >
                  {setupReady ? "Ready" : "Setup needed"}
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setChatManagerOpen(false);
                    setShowBrowserSurface(false);
                  }}
                  className={secondaryButtonClass("sm")}
                >
                  Hide
                </button>
              </div>
            </div>

            <div className="mt-4 space-y-4">
              <SetupSection title="1. ChatGPT account">
                <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                  <div className="min-w-0 text-sm leading-6 text-[var(--color-text-secondary)]">
                    <span className="font-semibold text-[var(--color-text-primary)]">
                      {browserReady ? "Signed in" : browserActive ? "Browser open" : "Browser not open"}
                    </span>
                    <span className="ml-2 text-xs text-[var(--color-text-tertiary)]">
                      {browserReady ? "CCCC can use this browser profile for ChatGPT delivery." : "Open ChatGPT here and sign in once."}
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
                      Open ChatGPT
                    </button>
                  </div>
                </div>
              </SetupSection>

              <SetupSection title="2. Delivery target">
                <div className="rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <div className="text-xs font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                      Delivery target
                    </div>
                    <span className={["inline-flex rounded-full border px-2 py-0.5 text-xs font-semibold", setupPillClass(targetModeTone)].join(" ")}>
                      {targetModeLabel}
                    </span>
                  </div>
                  <div className="mt-2 text-sm leading-6 text-[var(--color-text-secondary)]">
                    {boundConversationUrl ? (
                      <span>
                        Future messages go to{" "}
                        <span className="break-all font-mono text-[var(--color-text-primary)]">{targetModeDetail}</span>
                        {" "}until you choose another target.
                      </span>
                    ) : (
                      <span>{targetModeDetail}</span>
                    )}
                  </div>
                  {boundConversationUrl && currentBrowserUrl && currentBrowserUrl !== boundConversationUrl ? (
                    <div className="mt-1 text-xs leading-5 text-[var(--color-text-tertiary)]">
                      The browser is currently at {currentBrowserConversationUrl ? shortConversationLabel(currentBrowserConversationUrl) : "ChatGPT home"}; delivery still uses the bound target above.
                    </div>
                  ) : null}
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    onClick={() => void bindConversation(currentBrowserConversationUrl)}
                    disabled={browserBusy || !currentBrowserConversationUrl || !groupId || !actorId}
                    className={currentBrowserConversationUrl && !boundConversationUrl ? primaryButtonClass(browserBusy) : secondaryButtonClass("sm")}
                  >
                    Use current ChatGPT chat
                  </button>
                  <button
                    type="button"
                    onClick={() => void startNewConversationAutoBind()}
                    disabled={browserBusy || !groupId || !actorId}
                    className={!boundConversationUrl && !pendingNewChatBind ? primaryButtonClass(browserBusy) : secondaryButtonClass("sm")}
                  >
                    {pendingNewChatBind ? "New ChatGPT chat selected" : "Start a new ChatGPT chat"}
                  </button>
                </div>
                <details className="mt-3 text-xs leading-5 text-[var(--color-text-tertiary)]">
                  <summary className="cursor-pointer font-semibold text-[var(--color-text-secondary)]">Manual fallback: paste a ChatGPT URL</summary>
                  <div className="mt-2 grid gap-3 lg:grid-cols-[1fr_auto]">
                    <label className="block">
                      <span className={labelClass(isDark)}>ChatGPT conversation URL</span>
                      <input
                        value={conversationUrlDraft}
                        onChange={(event) => setConversationUrlDraft(event.target.value)}
                        placeholder="https://chatgpt.com/c/..."
                        className={inputClass(isDark)}
                      />
                    </label>
                    <div className="flex items-end">
                      <button
                        type="button"
                        onClick={() => void bindConversation(conversationUrlDraft)}
                        disabled={browserBusy || !groupId || !actorId || !isChatGptConversationUrl(conversationUrlDraft)}
                        className={secondaryButtonClass("sm")}
                      >
                        Save target URL
                      </button>
                    </div>
                  </div>
                </details>
              </SetupSection>

              {showBrowserSurface && groupId && actorId ? (
                <SetupSection title="Embedded ChatGPT browser">
                  <div className="mb-2 flex flex-col gap-2 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2 sm:flex-row sm:items-center sm:justify-between">
                    <div className="text-xs leading-5 text-[var(--color-text-tertiary)]">
                      Reload restarts the embedded ChatGPT browser while preserving this actor, group, browser profile, and delivery target.
                    </div>
                    <button
                      type="button"
                      onClick={() => void reloadEmbeddedBrowser()}
                      disabled={browserBusy || !groupId || !actorId}
                      className={secondaryButtonClass("sm")}
                    >
                      Reload ChatGPT page
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
                      starting: "Opening ChatGPT...",
                      waiting: "Waiting for ChatGPT...",
                      ready: "ChatGPT surface ready",
                      failed: "ChatGPT surface failed",
                      closed: "ChatGPT surface closed.",
                      reconnecting: "Reconnecting ChatGPT surface...",
                      reconnect: "Reconnect",
                      frameAlt: "ChatGPT browser frame",
                    }}
                  />
                </SetupSection>
              ) : null}

              <details className="text-xs leading-5 text-[var(--color-text-tertiary)]">
                <summary className="cursor-pointer font-semibold text-[var(--color-text-secondary)]">Advanced</summary>
                <div className="mt-2 grid gap-x-4 gap-y-1 sm:grid-cols-[140px_1fr]">
                  <span>Status</span>
                  <span>{browserStatusLabel} · {targetStatusLabel}</span>
                  <span>{browserActive ? "Current browser tab" : "Last browser tab"}</span>
                  <span className="break-all font-mono">{currentBrowserUrl || "none"}</span>
                  <span>Delivery target</span>
                  <span className="break-all font-mono">{boundConversationUrl || (pendingNewChatBind ? "new chat on next delivery" : "none")}</span>
                  {!boundConversationUrl && pendingNewChatBind ? (
                    <>
                      <span>Pending new chat</span>
                      <span className="break-all font-mono">{pendingNewChatUrl || "https://chatgpt.com/"}</span>
                    </>
                  ) : null}
                  {selectedBrowserSession?.profile_dir ? (
                    <>
                      <span>Profile</span>
                      <span className="break-all font-mono">{selectedBrowserSession.profile_dir}</span>
                    </>
                  ) : null}
                  {selectedBrowserSession?.visibility ? (
                    <>
                      <span>Mode</span>
                      <span>{selectedBrowserSession.visibility}</span>
                    </>
                  ) : null}
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    onClick={() => void checkBrowserSessionStatus()}
                    disabled={browserBusy || !groupId || !actorId}
                    className={secondaryButtonClass("sm")}
                  >
                    Check status
                  </button>
                  <button
                    type="button"
                    onClick={() => void closeBrowserSession()}
                    disabled={browserBusy || !browserActive}
                    className={secondaryButtonClass("sm")}
                  >
                    Close browser
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
