import { useCallback, useEffect, useMemo, useState } from "react";

import type { Actor } from "../../types";
import * as api from "../../services/api";
import type { WebModelBrowserSession } from "../../services/api";
import { classNames } from "../../utils/classNames";
import { formatTime } from "../../utils/time";
import { useModalStore } from "../../stores";
import { PlusIcon, RefreshIcon, SettingsIcon } from "../Icons";
import { ProjectedBrowserSurfacePanel } from "../browser/ProjectedBrowserSurfacePanel";

type Tone = "ready" | "needs" | "neutral" | "error";

type StatusBlock = {
  label: string;
  value: string;
  detail: string;
  tone: Tone;
};

interface WebModelRuntimePanelProps {
  groupId: string;
  actor: Actor;
  isDark: boolean;
  isVisible: boolean;
  isRunning: boolean;
  readOnly?: boolean;
}

function tonePillClass(tone: Tone): string {
  switch (tone) {
    case "ready":
      return "border-emerald-500/25 bg-emerald-500/12 text-emerald-700 dark:text-emerald-300";
    case "needs":
      return "border-amber-500/30 bg-amber-500/12 text-amber-700 dark:text-amber-300";
    case "error":
      return "border-rose-500/30 bg-rose-500/12 text-rose-700 dark:text-rose-300";
    case "neutral":
    default:
      return "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
  }
}

function shortChatGptUrl(value?: string): string {
  const raw = String(value || "").trim();
  if (!raw) return "";
  try {
    const parsed = new URL(raw);
    const parts = parsed.pathname.split("/").filter(Boolean);
    const chatId = parts[0] === "c" ? parts[1] || "" : "";
    if (chatId) return `${parsed.hostname}/c/${chatId.slice(0, 8)}...`;
    return parsed.hostname || raw;
  } catch {
    return raw.length > 42 ? `${raw.slice(0, 39)}...` : raw;
  }
}

function iconButtonClass(primary = false): string {
  return classNames(
    "inline-flex h-10 w-10 items-center justify-center rounded-xl border text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35 disabled:cursor-not-allowed disabled:opacity-50",
    primary
      ? "border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white hover:bg-black dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white/92"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
  );
}

function buildChatGptBlock(session: WebModelBrowserSession | null): StatusBlock {
  const error = String(session?.error || "").trim();
  if (error) {
    return {
      label: "ChatGPT",
      value: "Check failed",
      detail: error,
      tone: "error",
    };
  }
  if (session?.ready) {
    return {
      label: "ChatGPT",
      value: "Ready",
      detail: "Signed in and reachable.",
      tone: "ready",
    };
  }
  if (session?.active) {
    return {
      label: "ChatGPT",
      value: "Needs sign-in",
      detail: shortChatGptUrl(session.tab_url || session.last_tab_url) || "Browser is open.",
      tone: "needs",
    };
  }
  return {
    label: "ChatGPT",
    value: "Not open",
    detail: "Open settings to sign in or inspect the page.",
    tone: "neutral",
  };
}

function buildTargetBlock(session: WebModelBrowserSession | null): StatusBlock {
  const conversationUrl = String(session?.conversation_url || "").trim();
  if (conversationUrl) {
    return {
      label: "Target",
      value: "Bound chat",
      detail: shortChatGptUrl(conversationUrl),
      tone: "ready",
    };
  }
  if (session?.pending_new_chat_bind) {
    return {
      label: "Target",
      value: "New chat next",
      detail: "Next delivery creates and binds a ChatGPT conversation.",
      tone: "ready",
    };
  }
  return {
    label: "Target",
    value: "Not selected",
    detail: "Choose a target chat in settings.",
    tone: "needs",
  };
}

function buildActivityBlock(session: WebModelBrowserSession | null, queuedCount: number): StatusBlock {
  if (queuedCount > 0) {
    return {
      label: "Activity",
      value: `${queuedCount} queued`,
      detail: "Waiting for browser delivery.",
      tone: "needs",
    };
  }
  if (session?.last_delivery_at) {
    return {
      label: "Activity",
      value: `Last ${formatTime(session.last_delivery_at)}`,
      detail: session.last_turn_id ? String(session.last_turn_id) : "Browser delivery completed.",
      tone: "neutral",
    };
  }
  return {
    label: "Activity",
    value: "No recent delivery",
    detail: "This actor has no browser delivery record yet.",
    tone: "neutral",
  };
}

function StatusCell({ block }: { block: StatusBlock }) {
  return (
    <div className="min-w-0 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3">
      <div className="flex min-w-0 items-center justify-between gap-2">
        <div className="truncate text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
          {block.label}
        </div>
        <span className={classNames("shrink-0 rounded-full border px-2 py-0.5 text-[11px] font-semibold", tonePillClass(block.tone))}>
          {block.value}
        </span>
      </div>
      <div className="mt-2 truncate text-xs leading-5 text-[var(--color-text-tertiary)]" title={block.detail}>
        {block.detail}
      </div>
    </div>
  );
}

export function WebModelRuntimePanel({
  groupId,
  actor,
  isDark,
  isVisible,
  isRunning,
  readOnly,
}: WebModelRuntimePanelProps) {
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const [session, setSession] = useState<WebModelBrowserSession | null>(null);
  const [error, setError] = useState("");
  const [busyAction, setBusyAction] = useState("");
  const [surfaceRefreshNonce, setSurfaceRefreshNonce] = useState(0);
  const actorId = String(actor.id || "").trim();
  const queuedCount = Math.max(0, Number(actor.web_model_queued_count || 0));
  const canControlSurface = Boolean(isVisible && isRunning && !readOnly && groupId && actorId);

  const loadSession = useCallback(async () => {
    if (!groupId || !actorId) return null;
    const resp = await api.fetchWebModelBrowserSession(groupId, actorId);
    if (!resp.ok) {
      const message = resp.error?.message || "Failed to load ChatGPT browser status.";
      setError(message);
      return null;
    }
    const next = resp.result.browser_session || {};
    setSession(next);
    setError("");
    return next;
  }, [actorId, groupId]);

  useEffect(() => {
    if (!isVisible || !groupId || !actorId) return;
    let cancelled = false;
    setBusyAction("load");
    void api.fetchWebModelBrowserSession(groupId, actorId)
      .then((resp) => {
        if (cancelled) return;
        if (!resp.ok) {
          setError(resp.error?.message || "Failed to load ChatGPT browser status.");
          return;
        }
        setSession(resp.result.browser_session || {});
        setError("");
      })
      .catch(() => {
        if (!cancelled) setError("Failed to load ChatGPT browser status.");
      })
      .finally(() => {
        if (!cancelled) setBusyAction("");
      });
    return () => {
      cancelled = true;
    };
  }, [actorId, groupId, isVisible]);

  const reloadChatGptPage = async () => {
    if (!groupId || !actorId) return;
    if (!canControlSurface) {
      const message = readOnly
        ? "ChatGPT page reload is disabled in read-only mode."
        : "Start this actor to reload the embedded ChatGPT page.";
      setError(message);
      return;
    }
    setBusyAction("reload");
    setError("");
    try {
      setSurfaceRefreshNonce((value) => value + 1);
      await loadSession();
    } finally {
      setBusyAction("");
    }
  };

  const useNewChat = async () => {
    if (readOnly || !groupId || !actorId) return;
    setBusyAction("new-chat");
    setError("");
    try {
      const resp = await api.bindCurrentWebModelBrowserConversation({
        groupId,
        actorId,
        conversationUrl: "https://chatgpt.com/",
        newChat: true,
      });
      if (resp.ok) {
        setSession(resp.result.browser_session || {});
      } else {
        setError(resp.error?.message || "Failed to select a new ChatGPT chat.");
      }
    } catch {
      setError("Failed to select a new ChatGPT chat.");
    } finally {
      setBusyAction("");
    }
  };

  const openSettings = () => {
    openSettingsTarget({ scope: "global", tab: "webModels" });
  };

  const loadBrowserSurfaceSession = useCallback(async () => {
    const resp = await api.fetchWebModelBrowserSurfaceSession(groupId, actorId);
    if (resp.ok) {
      setSession(resp.result.browser_session || {});
      setError("");
    } else {
      setError(resp.error?.message || "Failed to load ChatGPT browser surface.");
    }
    return resp;
  }, [actorId, groupId]);

  const startBrowserSurfaceSession = useCallback(async ({ width, height }: { width: number; height: number }) => {
    if (!canControlSurface) {
      const message = readOnly
        ? "ChatGPT browser control is disabled in read-only mode."
        : "Start this actor to open the embedded ChatGPT browser.";
      setError(message);
      return {
        ok: false as const,
        error: { code: "browser_surface_unavailable", message, details: {} },
      };
    }
    const resp = await api.openWebModelBrowserSurfaceSession({ groupId, actorId, width, height });
    if (resp.ok) {
      setSession(resp.result.browser_session || {});
      setError("");
    } else {
      setError(resp.error?.message || "Failed to open ChatGPT browser surface.");
    }
    return resp;
  }, [actorId, canControlSurface, groupId, readOnly]);

  const blocks = useMemo(
    () => [
      buildChatGptBlock(session),
      buildTargetBlock(session),
      buildActivityBlock(session, queuedCount),
    ],
    [queuedCount, session],
  );
  const primaryActionNeeded = !session?.ready || (!session?.conversation_url && !session?.pending_new_chat_bind);
  const showNewChatAction = !readOnly && !session?.conversation_url && !session?.pending_new_chat_bind;
  const surfaceDisabledMessage = readOnly
    ? "Browser view is unavailable in read-only mode."
    : !isRunning
      ? "Start this actor to inspect the live ChatGPT browser here."
      : "";

  return (
    <section
      className={classNames(
        "flex min-h-0 flex-1 flex-col rounded-[20px] border px-3 py-3 shadow-[0_20px_70px_-55px_rgba(15,23,42,0.65)] sm:px-4",
        isDark
          ? "border-white/10 bg-[linear-gradient(180deg,rgba(24,26,31,0.92),rgba(13,14,18,0.98))]"
          : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.995),rgba(246,248,251,0.96))]",
      )}
      aria-label="ChatGPT Web Model runtime"
    >
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="min-w-0">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <div className="truncate text-sm font-semibold text-[var(--color-text-primary)]">ChatGPT Web Model</div>
            <span className={classNames("rounded-full border px-2 py-0.5 text-[11px] font-semibold", tonePillClass(isRunning ? "ready" : "neutral"))}>
              {isRunning ? "Actor running" : "Actor stopped"}
            </span>
          </div>
          <div className="mt-1 text-xs leading-5 text-[var(--color-text-tertiary)]">
            Browser delivery uses the bound ChatGPT conversation below.
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            onClick={reloadChatGptPage}
            disabled={Boolean(busyAction)}
            className={iconButtonClass(false)}
            title="Reload ChatGPT page"
            aria-label="Reload ChatGPT page"
          >
            <RefreshIcon size={17} aria-hidden="true" />
          </button>
          {showNewChatAction ? (
            <button
              type="button"
              onClick={useNewChat}
              disabled={Boolean(busyAction)}
              className={iconButtonClass(false)}
              title="Use a new ChatGPT chat on next delivery"
              aria-label="Use a new ChatGPT chat on next delivery"
            >
              <PlusIcon size={17} aria-hidden="true" />
            </button>
          ) : null}
          <button
            type="button"
            onClick={openSettings}
            className={iconButtonClass(primaryActionNeeded)}
            title="Open ChatGPT Web Model settings"
            aria-label="Open ChatGPT Web Model settings"
          >
            <SettingsIcon size={17} aria-hidden="true" />
          </button>
        </div>
      </div>

      <div className="mt-3 grid gap-2 md:grid-cols-3">
        {blocks.map((block) => (
          <StatusCell key={block.label} block={block} />
        ))}
      </div>

      {canControlSurface ? (
        <div className="mt-3 min-h-0 flex-1 overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)]">
          <ProjectedBrowserSurfacePanel
            isDark={isDark}
            refreshNonce={surfaceRefreshNonce}
            chromeMode="embedded"
            viewportClassName="h-full min-h-0"
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
      ) : surfaceDisabledMessage ? (
        <div className="mt-3 flex min-h-[240px] flex-1 items-center justify-center rounded-2xl border border-dashed border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3 text-center text-xs leading-5 text-[var(--color-text-tertiary)]">
          {surfaceDisabledMessage}
        </div>
      ) : null}

      {error ? (
        <div className="mt-2 rounded-xl border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-xs leading-5 text-rose-700 dark:text-rose-300">
          {error}
        </div>
      ) : null}
    </section>
  );
}
