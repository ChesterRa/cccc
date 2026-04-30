import { useCallback, useEffect, useMemo, useState } from "react";
import type { Actor, GroupMeta, RemoteAccessState } from "../../../types";
import * as api from "../../../services/api";
import { copyTextToClipboard } from "../../../utils/copy";
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

function SetupStep({
  label,
  detail,
  tone,
}: {
  label: string;
  detail: string;
  tone: SetupTone;
}) {
  return (
    <div className={["rounded-lg border px-2.5 py-2", setupPillClass(tone)].join(" ")}>
      <div className="text-[11px] font-semibold">{label}</div>
      <div className="mt-0.5 text-[11px] leading-4 opacity-85">{detail}</div>
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
  const [groupId, setGroupId] = useState(currentGroupId);
  const [actorId, setActorId] = useState("");
  const [busy, setBusy] = useState(false);
  const [createBusy, setCreateBusy] = useState(false);
  const [startBusyId, setStartBusyId] = useState("");
  const [revokeBusyId, setRevokeBusyId] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [browserSession, setBrowserSession] = useState<api.WebModelBrowserSession | null>(null);
  const [browserSessionsByActor, setBrowserSessionsByActor] = useState<Record<string, api.WebModelBrowserSession>>({});
  const [browserBusy, setBrowserBusy] = useState(false);
  const [showBrowserSurface, setShowBrowserSurface] = useState(false);
  const [chatManagerOpen, setChatManagerOpen] = useState(false);
  const [browserSurfaceRefreshNonce, setBrowserSurfaceRefreshNonce] = useState(0);
  const [conversationUrlDraft, setConversationUrlDraft] = useState("");

  const webModelActors = useMemo(
    () => actors.filter((actor) => String(actor.runtime || "").trim().toLowerCase() === "web_model"),
    [actors],
  );

  const activeConnectors = useMemo(
    () => connectors.filter((connector) => !connector.revoked),
    [connectors],
  );
  const revokedConnectors = useMemo(
    () => connectors.filter((connector) => connector.revoked),
    [connectors],
  );
  const currentGroupActiveConnectors = useMemo(
    () => activeConnectors.filter((connector) => String(connector.group_id || "") === groupId),
    [activeConnectors, groupId],
  );
  const currentGroupRevokedConnectors = useMemo(
    () => revokedConnectors.filter((connector) => String(connector.group_id || "") === groupId),
    [revokedConnectors, groupId],
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

  const configuredPublicUrl = String(remoteState?.config?.web_public_url || remoteState?.diagnostics?.web_public_url || "").trim();
  const publicEndpointReady = Boolean(configuredPublicUrl && isHttpsUrl(configuredPublicUrl));
  const uiAccessTokenPresent = Boolean(remoteState?.config?.access_token_configured || remoteState?.diagnostics?.access_token_present);
  const selectedBrowserSession = browserSessionsByActor[actorId] || browserSession || null;
  const browserActive = Boolean(selectedBrowserSession?.active || showBrowserSurface);
  const browserReady = Boolean(selectedBrowserSession?.ready);
  const boundConversationUrl = String(selectedBrowserSession?.conversation_url || "").trim();
  const currentBrowserUrl = String(selectedBrowserSession?.tab_url || selectedBrowserSession?.last_tab_url || "").trim();
  const browserStatusLabel = browserReady
    ? "ChatGPT ready"
    : browserActive
      ? "Sign-in needed"
      : "Not open";
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
      setError(resp.error?.message || "Failed to load web-model connectors.");
    }
  }, []);

  const loadBrowserSession = useCallback(async (gid: string = groupId, aid: string = actorId) => {
    if (!gid || !aid) {
      setBrowserSession(null);
      return;
    }
    const resp = await api.fetchWebModelBrowserSession(gid, aid);
    if (resp.ok) {
      const nextSession = resp.result?.browser_session || null;
      setBrowserSessionsByActor((current) => ({
        ...current,
        [aid]: nextSession || {},
      }));
      if (aid === actorId) setBrowserSession(nextSession);
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
      if (entry) next[entry[0]] = entry[1];
    }
    setBrowserSessionsByActor(next);
    if (actorId && next[actorId]) setBrowserSession(next[actorId]);
  }, [actorId]);

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
    if (!groupId || !actorId) {
      setBrowserSession(null);
      return {
        ok: true as const,
        result: {
          browser_session: {},
          browser_surface: api.normalizePresentationBrowserSurfaceState(null),
        },
      };
    }
    const resp = await api.fetchWebModelBrowserSurfaceSession(groupId, actorId);
    if (resp.ok) {
      const nextSession = resp.result.browser_session || null;
      setBrowserSession(nextSession);
      setBrowserSessionsByActor((current) => ({
        ...current,
        [actorId]: nextSession || {},
      }));
    } else {
      setError(resp.error?.message || "Failed to load ChatGPT browser session.");
    }
    return resp;
  }, [actorId, groupId]);

  const startBrowserSurfaceSession = useCallback(async (size: { width: number; height: number }) => {
    if (!groupId || !actorId) {
      return {
        ok: false as const,
        error: {
          code: "missing_actor",
          message: "Select a group and a Browser Web Model actor first.",
        },
      };
    }
    const resp = await api.openWebModelBrowserSurfaceSession({
      groupId,
      actorId,
      width: size.width,
      height: size.height,
    });
    if (resp.ok) {
      const nextSession = resp.result.browser_session || null;
      setBrowserSession(nextSession);
      setBrowserSessionsByActor((current) => ({
        ...current,
        [actorId]: nextSession || {},
      }));
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
        setGroupId((current) => {
          if (current && nextGroups.some((group) => group.group_id === current)) return current;
          if (currentGroupId && nextGroups.some((group) => group.group_id === currentGroupId)) return currentGroupId;
          return nextGroups[0]?.group_id || "";
        });
      } else {
        setError(groupsResp.error?.message || "Failed to load groups.");
      }
    } catch {
      setError("Failed to load web-model connector settings.");
    } finally {
      setBusy(false);
    }
  }, [currentGroupId, isActive, loadConnectors]);

  useEffect(() => {
    void loadInitial();
  }, [loadInitial]);

  useEffect(() => {
    if (!isActive || !groupId) {
      setActors([]);
      setActorId("");
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
    if (!isActive || !groupId || !actorId) {
      setBrowserSession(null);
      setShowBrowserSurface(false);
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

  const createConnector = async (targetActorId = actorId) => {
    const aid = String(targetActorId || "").trim();
    if (!groupId || !aid) {
      setError("Select a group and a Browser Web Model actor first.");
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
    if (!groupId || !actorId) {
      setError("Select a group and a Browser Web Model actor first.");
      return;
    }
    setError("");
    setChatManagerOpen(true);
    setShowBrowserSurface(true);
    setBrowserSurfaceRefreshNonce((value) => value + 1);
    pushNotice("ChatGPT sign-in surface opened below.");
  };

  const refreshBrowserSession = async () => {
    setBrowserBusy(true);
    setError("");
    try {
      if (showBrowserSurface) {
        setBrowserSurfaceRefreshNonce((value) => value + 1);
        await loadBrowserSurfaceSession();
      } else {
        await loadBrowserSession();
      }
    } finally {
      setBrowserBusy(false);
    }
  };

  const closeBrowserSession = async () => {
    if (!groupId || !actorId) return;
    setBrowserBusy(true);
    setError("");
    try {
      const resp = await api.closeWebModelBrowserSurfaceSession(groupId, actorId);
      if (resp.ok) {
        const nextSession = resp.result?.browser_session || null;
        setBrowserSession(nextSession);
        setBrowserSessionsByActor((current) => ({
          ...current,
          [actorId]: nextSession || {},
        }));
        setShowBrowserSurface(false);
        pushNotice("ChatGPT browser session closed.");
      } else {
        setError(resp.error?.message || "Failed to close ChatGPT browser session.");
      }
    } catch {
      setError("Failed to close ChatGPT browser session.");
    } finally {
      setBrowserBusy(false);
    }
  };

  const bindConversation = async (conversationUrl = "") => {
    if (!groupId || !actorId) return;
    setBrowserBusy(true);
    setError("");
    try {
      const resp = await api.bindCurrentWebModelBrowserConversation({ groupId, actorId, conversationUrl });
      if (resp.ok) {
        const nextSession = resp.result?.browser_session || null;
        setBrowserSession(nextSession);
        setBrowserSessionsByActor((current) => ({
          ...current,
          [actorId]: nextSession || {},
        }));
        setConversationUrlDraft("");
        pushNotice("ChatGPT conversation bound to this actor.");
      } else {
        setError(resp.error?.message || "Failed to bind ChatGPT conversation.");
      }
    } catch {
      setError("Failed to bind ChatGPT conversation.");
    } finally {
      setBrowserBusy(false);
    }
  };

  const clearConversation = async () => {
    if (!groupId || !actorId) return;
    setBrowserBusy(true);
    setError("");
    try {
      const resp = await api.bindCurrentWebModelBrowserConversation({ groupId, actorId, clear: true });
      if (resp.ok) {
        const nextSession = resp.result?.browser_session || null;
        setBrowserSession(nextSession);
        setBrowserSessionsByActor((current) => ({
          ...current,
          [actorId]: nextSession || {},
        }));
        pushNotice("ChatGPT conversation binding cleared.");
      } else {
        setError(resp.error?.message || "Failed to clear ChatGPT conversation binding.");
      }
    } catch {
      setError("Failed to clear ChatGPT conversation binding.");
    } finally {
      setBrowserBusy(false);
    }
  };

  const manageActorChat = (aid: string) => {
    const nextActorId = String(aid || "").trim();
    if (!nextActorId) return;
    setActorId(nextActorId);
    setBrowserSession(browserSessionsByActor[nextActorId] || null);
    setChatManagerOpen(true);
    setShowBrowserSurface(false);
    setConversationUrlDraft(String(browserSessionsByActor[nextActorId]?.conversation_url || ""));
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
            Browser Runtime
          </div>
          <h3 className="mt-1 text-base font-semibold text-[var(--color-text-primary)]">Web Model actors</h3>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-[var(--color-text-tertiary)]">
            Each Browser Web Model actor owns one active remote MCP connector. Create another actor for another browser chat or model;
            rotating a connector revokes the previous credential for that actor.
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
          <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
            <div className="min-w-0">
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">Public endpoint readiness</div>
              <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
                ChatGPT calls this MCP server from its cloud. Use a public HTTPS Web URL; the copied MCP URL already carries the actor connector token, so ChatGPT Authentication stays No Auth.
              </p>
              <div className="mt-2 break-all font-mono text-xs text-[var(--color-text-primary)]">
                {configuredPublicUrl || "No public Web URL configured"}
              </div>
            </div>
            <div className="flex shrink-0 flex-wrap items-start gap-2 lg:justify-end">
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
                {uiAccessTokenPresent ? "CCCC UI token present" : "CCCC UI token optional"}
              </span>
              {onOpenWebAccess ? (
                <button type="button" onClick={onOpenWebAccess} className={secondaryButtonClass("sm")}>
                  Open Web Access
                </button>
              ) : null}
            </div>
          </div>
        </section>

        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="grid gap-3 lg:grid-cols-[minmax(280px,420px)_1fr]">
            <label className="block">
              <span className={labelClass(isDark)}>Group</span>
              <select value={groupId} onChange={(event) => setGroupId(event.target.value)} className={inputClass(isDark)}>
                {groups.map((group) => (
                  <option key={group.group_id} value={group.group_id}>
                    {group.title || group.group_id}
                  </option>
                ))}
              </select>
            </label>
            <div className="flex items-end text-xs leading-5 text-[var(--color-text-tertiary)]">
              Configure one actor at a time. Each actor owns one ChatGPT app URL and one bound ChatGPT conversation.
            </div>
          </div>
        </section>

        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">Web Model actors</div>
              <div className="mt-1 text-xs text-[var(--color-text-tertiary)]">
                {actorRows.length} actors · {currentGroupActiveConnectors.length} connected · each actor needs one MCP URL and one bound ChatGPT chat
              </div>
            </div>
          </div>
          <div className="mt-3 divide-y divide-[var(--glass-border-subtle)]">
            {actorRows.length ? actorRows.map(({ actor, connector }) => {
              const queuedCount = webModelQueuedCount(actor);
              const mcpUrl = connectorMcpUrl(connector || null);
              const connectorUrl = String(connector?.connector_url || "").trim();
              const rowLocalWarning = Boolean(mcpUrl || connectorUrl) && isLocalConnectorUrl(mcpUrl || connectorUrl);
              const rowHttpsWarning = Boolean(mcpUrl || connectorUrl) && !isHttpsUrl(mcpUrl || connectorUrl);
              const actorLabel = String(actor.title || actor.id || "").trim() || actor.id;
              const selected = actor.id === actorId;
              const rowSession = browserSessionsByActor[actor.id] || {};
              const targetUrl = String(rowSession.conversation_url || "").trim();
              const targetReady = Boolean(targetUrl);
              const actorRunning = Boolean(actor.running);
              const chatGptSeen = Boolean(connector?.last_activity_at);
              const browserLoginReady = Boolean(rowSession.ready);
              const setupSteps = [
                {
                  label: "Public URL",
                  detail: publicEndpointReady ? "HTTPS ready" : "Set HTTPS URL in Web Access",
                  tone: publicEndpointReady ? "ready" : "needs",
                },
                {
                  label: "Actor",
                  detail: actorRunning ? "Running" : "Start this actor",
                  tone: actorRunning ? "ready" : "needs",
                },
                {
                  label: "MCP URL",
                  detail: mcpUrl ? "Copy into ChatGPT" : connector ? "Rotate connector" : "Create connector",
                  tone: mcpUrl ? "ready" : "needs",
                },
                {
                  label: "ChatGPT app",
                  detail: chatGptSeen ? `Seen ${formatTime(connector?.last_activity_at)}` : "Create app in ChatGPT",
                  tone: chatGptSeen ? "ready" : "needs",
                },
                {
                  label: "Target chat",
                  detail: targetReady ? shortConversationLabel(targetUrl) : "Bind chatgpt.com/c/...",
                  tone: targetReady ? "ready" : "needs",
                },
                {
                  label: "Browser login",
                  detail: browserLoginReady ? "Signed in" : "Open browser and sign in",
                  tone: browserLoginReady ? "ready" : "needs",
                },
              ] satisfies Array<{ label: string; detail: string; tone: SetupTone }>;
              const readyCount = setupSteps.filter((step) => step.tone === "ready").length;
              const nextAction = !publicEndpointReady
                ? "Set the public HTTPS URL in Web Access."
                : !actorRunning
                  ? "Start this Web Model actor."
                  : !mcpUrl
                    ? connector
                      ? "Rotate the connector once to generate the ChatGPT MCP URL."
                      : "Create this actor's connector."
                    : !chatGptSeen
                      ? "Paste the MCP URL into ChatGPT New App with Authentication set to No Auth."
                      : !targetReady
                        ? "Bind a concrete ChatGPT conversation URL."
                        : !browserLoginReady
                          ? "Open the embedded browser and sign in to ChatGPT."
                          : "Ready for browser delivery.";
              return (
                <div key={actor.id} className="py-3 first:pt-0 last:pb-0">
                  <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2 text-sm font-semibold text-[var(--color-text-primary)]">
                        <span>{actorLabel}</span>
                        <span
                          className={[
                            "rounded-full border px-2 py-0.5 text-[11px] font-medium",
                            readyCount === setupSteps.length
                              ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                              : "border-amber-500/25 bg-amber-500/10 text-amber-700 dark:text-amber-300",
                          ].join(" ")}
                        >
                          {readyCount}/{setupSteps.length} ready
                        </span>
                        {queuedCount > 0 ? (
                          <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2 py-0.5 text-[11px] font-medium text-amber-700 dark:text-amber-200">
                            {queuedCount} queued for next turn
                          </span>
                        ) : null}
                        {selected ? (
                          <span className="rounded-full border border-[var(--glass-border-subtle)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)]">
                            selected
                          </span>
                        ) : null}
                      </div>
                      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-[var(--color-text-tertiary)]">
                        {connector?.provider ? <span>{connector.provider}</span> : null}
                        {connector ? <span>remote={connectorActivityLabel(connector)}</span> : null}
                        {targetUrl ? <span className="break-all font-mono">chat={shortConversationLabel(targetUrl)}</span> : null}
                        {rowSession.last_turn_id ? <span className="font-mono">last turn={rowSession.last_turn_id}</span> : null}
                        {connector?.last_error ? <span className="text-rose-600 dark:text-rose-300">{connector.last_error}</span> : null}
                      </div>
                      <div className="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4">
                        {setupSteps.map((step) => (
                          <SetupStep key={step.label} label={step.label} detail={step.detail} tone={step.tone} />
                        ))}
                      </div>
                      <div className="mt-2 text-xs leading-5 text-[var(--color-text-secondary)]">
                        Next: {nextAction}
                      </div>
                      {mcpUrl ? (
                        <div className="mt-3 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2 text-xs leading-5 text-[var(--color-text-tertiary)]">
                          <div className="font-semibold text-[var(--color-text-primary)]">ChatGPT New App fields</div>
                          <div className="mt-1 grid gap-x-3 gap-y-1 sm:grid-cols-[110px_1fr]">
                            <span>Name</span>
                            <span className="font-mono text-[var(--color-text-primary)]">CCCC - {actorLabel}</span>
                            <span>Description</span>
                            <span>Local CCCC agent connector for {actorLabel}.</span>
                            <span>MCP Server URL</span>
                            <span>Use Copy MCP URL.</span>
                            <span>Authentication</span>
                            <span>No Auth</span>
                          </div>
                          <div className="mt-1">Then check “I understand and want to continue” and click Create.</div>
                        </div>
                      ) : null}
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
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-2 xl:justify-end">
                      {!actorRunning ? (
                        <button
                          type="button"
                          onClick={() => void startWebModelActor(actor.id)}
                          disabled={startBusyId === actor.id}
                          className={secondaryButtonClass("sm")}
                        >
                          Start
                        </button>
                      ) : null}
                      <button
                        type="button"
                        onClick={() => void copyValue(mcpUrl, "MCP URL")}
                        disabled={!mcpUrl}
                        className={secondaryButtonClass("sm")}
                      >
                        Copy MCP URL
                      </button>
                      <button
                        type="button"
                        onClick={() => void createConnector(actor.id)}
                        disabled={createBusy || !groupId}
                        className={secondaryButtonClass("sm")}
                      >
                        {connector ? "Rotate" : "Create"}
                      </button>
                      <button
                        type="button"
                        onClick={() => manageActorChat(actor.id)}
                        className={primaryButtonClass(false)}
                      >
                        Bind chat
                      </button>
                      {connector ? (
                        <button
                          type="button"
                          onClick={() => void revokeConnector(connector.connector_id)}
                          disabled={revokeBusyId === connector.connector_id}
                          className={dangerButtonClass("sm")}
                        >
                          Revoke
                        </button>
                      ) : null}
                    </div>
                  </div>
                </div>
              );
            }) : (
              <div className="rounded-lg border border-dashed border-[var(--glass-border-subtle)] px-3 py-5 text-sm text-[var(--color-text-tertiary)]">
                Add an actor with runtime “Browser Web Model” first, then start that actor before browser delivery or pull-mode polling begins.
              </div>
            )}
          </div>
        </section>

        {chatManagerOpen && selectedActor ? (
          <section className={settingsWorkspacePanelClass(isDark)}>
            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div className="min-w-0">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  Manage ChatGPT chat for {selectedActorLabel || actorId}
                </div>
                <p className="mt-1 text-sm leading-6 text-[var(--color-text-tertiary)]">
                  Paste a ChatGPT conversation URL as the normal path. Open the embedded browser only for sign-in or to bind the currently visible chat.
                </p>
                <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-[var(--color-text-tertiary)]">
                  {selectedBrowserSession?.profile_dir ? <span className="break-all font-mono">profile={selectedBrowserSession.profile_dir}</span> : null}
                  {selectedBrowserSession?.visibility ? <span>mode={selectedBrowserSession.visibility}</span> : null}
                  {boundConversationUrl ? <span className="break-all font-mono">bound={boundConversationUrl}</span> : <span>bound=none</span>}
                  {currentBrowserUrl ? <span className="break-all font-mono">current={currentBrowserUrl}</span> : null}
                </div>
                {selectedBrowserSession?.error ? (
                  <div className="mt-2 text-xs leading-5 text-rose-600 dark:text-rose-300">{selectedBrowserSession.error}</div>
                ) : null}
              </div>
              <div className="flex shrink-0 flex-col items-start gap-2 sm:items-end">
                <span
                  className={[
                    "inline-flex items-center rounded-full border px-3 py-1 text-xs font-semibold",
                    boundConversationUrl
                      ? "border-emerald-500/30 bg-emerald-500/12 text-emerald-700 dark:text-emerald-300"
                      : "border-amber-500/30 bg-amber-500/12 text-amber-700 dark:text-amber-300",
                  ].join(" ")}
                >
                  {boundConversationUrl ? "Target chat bound" : "Target chat required"}
                </span>
                <span
                  className={[
                    "inline-flex items-center rounded-full border px-3 py-1 text-xs font-semibold",
                    browserReady
                      ? "border-emerald-500/30 bg-emerald-500/12 text-emerald-700 dark:text-emerald-300"
                      : browserActive
                        ? "border-amber-500/30 bg-amber-500/12 text-amber-700 dark:text-amber-300"
                        : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]",
                  ].join(" ")}
                >
                  {browserStatusLabel}
                </span>
              </div>
            </div>
            <div className="mt-3 grid gap-3 lg:grid-cols-[1fr_auto_auto]">
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
                  disabled={browserBusy || !groupId || !actorId || !conversationUrlDraft.trim()}
                  className={primaryButtonClass(browserBusy)}
                >
                  Bind URL
                </button>
              </div>
              <div className="flex items-end">
                <button
                  type="button"
                  onClick={() => void clearConversation()}
                  disabled={browserBusy || !boundConversationUrl}
                  className={secondaryButtonClass("sm")}
                >
                  Clear
                </button>
              </div>
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                type="button"
                onClick={() => void openBrowserLogin()}
                disabled={browserBusy || !groupId || !actorId}
                className={secondaryButtonClass("sm")}
              >
                Open embedded browser
              </button>
              <button
                type="button"
                onClick={() => void refreshBrowserSession()}
                disabled={browserBusy || !groupId || !actorId}
                className={secondaryButtonClass("sm")}
              >
                Check
              </button>
              <button
                type="button"
                onClick={() => void bindConversation()}
                disabled={browserBusy || !browserActive || !groupId || !actorId}
                className={secondaryButtonClass("sm")}
              >
                Bind current tab
              </button>
              <button
                type="button"
                onClick={() => void closeBrowserSession()}
                disabled={browserBusy || !browserActive}
                className={secondaryButtonClass("sm")}
              >
                Close browser
              </button>
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
            <div className="mt-2 text-xs leading-5 text-[var(--color-text-tertiary)]">
              Browser delivery will not guess a ChatGPT tab. Bind an explicit <span className="font-mono">chatgpt.com/c/...</span> URL for each actor.
            </div>
            {showBrowserSurface && groupId && actorId ? (
              <div className="mt-4">
                <ProjectedBrowserSurfacePanel
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
              </div>
            ) : null}
          </section>
        ) : null}

        {currentGroupRevokedConnectors.length ? (
          <section className={settingsWorkspacePanelClass(isDark)}>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">Revoked connector history</div>
            <div className="mt-3 space-y-2">
              {currentGroupRevokedConnectors.map((connector) => (
                <div key={connector.connector_id} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2 text-xs text-[var(--color-text-tertiary)]">
                  <span className="font-mono">{connector.connector_id}</span>
                  <span>{connector.actor_id}</span>
                  <span>{formatTime(connector.updated_at)}</span>
                </div>
              ))}
            </div>
          </section>
        ) : null}
      </div>
    </div>
  );
}
