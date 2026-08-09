import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Actor } from "../../types";
import * as api from "../../services/api";
import type { WebModelBrowserSession, WebModelDeliveryMode } from "../../services/api";
import { classNames } from "../../utils/classNames";
import { formatTime } from "../../utils/time";
import { matchesWebModelActorSelection } from "../../utils/webModelSelection";
import { useModalStore } from "../../stores";
import { HoverTooltip } from "../HoverTooltip";
import { InfoIcon, RefreshIcon, SettingsIcon } from "../Icons";
import { ProjectedBrowserSurfacePanel } from "../browser/ProjectedBrowserSurfacePanel";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { WebModelMcpShortcut } from "./WebModelMcpShortcut";

type Tone = "ready" | "needs" | "neutral" | "error";

type StatusBlock = { label: string; value: string; detail: string; tone: Tone };

interface WebModelRuntimePanelProps {
  groupId: string;
  actor: Actor;
  isRunning: boolean;
  isDark: boolean;
  isVisible: boolean;
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
  const health = session?.health_snapshot;
  if (health?.browser?.state) {
    const state = String(health.browser.state || "").trim();
    return {
      label: "ChatGPT",
      value:
        String(health.browser.label || "").trim() || (state === "ready" ? "Ready" : "Check status"),
      detail:
        String(health.browser.reason || health.browser.url || "").trim() ||
        "ChatGPT browser state.",
      tone:
        state === "ready"
          ? "ready"
          : state === "failed"
            ? "error"
            : state === "closed"
              ? "neutral"
              : "needs",
    };
  }
  const error = String(session?.error || "").trim();
  if (error) {
    return { label: "ChatGPT", value: "Check failed", detail: error, tone: "error" };
  }
  if (session?.ready) {
    return { label: "ChatGPT", value: "Ready", detail: "Signed in and reachable.", tone: "ready" };
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

function targetLabelValue(raw: string, state: string): string {
  const label = raw.trim();
  if (state === "missing") return "No target";
  if (state === "invalid") return "Rebind chat";
  if (state === "unavailable") return "Chat unavailable";
  if (state === "new_chat_pending") {
    if (label.toLowerCase().includes("binding")) return "Binding new chat";
    return "New chat next";
  }
  if (state === "bound") return "Existing chat";
  if (!label) return state ? "Target selected" : "No target";
  return label
    .replace(/\bChatGPT\b/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function buildTargetBlock(session: WebModelBrowserSession | null): StatusBlock {
  const health = session?.health_snapshot;
  if (health?.target?.state) {
    const state = String(health.target.state || "").trim();
    return {
      label: "Target",
      value: targetLabelValue(String(health.target.label || ""), state),
      detail: String(health.target.reason || "").trim() || "ChatGPT delivery target.",
      tone: ["missing", "invalid", "unavailable"].includes(state) ? "needs" : "ready",
    };
  }
  if (session?.conversation_url) {
    return {
      label: "Target",
      value: "Existing chat",
      detail: "Next delivery goes to the saved ChatGPT conversation.",
      tone: "ready",
    };
  }
  if (session?.pending_new_chat_bind) {
    return {
      label: "Target",
      value: "New chat next",
      detail: "Next delivery starts a fresh ChatGPT chat and binds it.",
      tone: "ready",
    };
  }
  return {
    label: "Target",
    value: "No target",
    detail: "Choose a target chat in settings.",
    tone: "needs",
  };
}

function buildActivityBlock(
  session: WebModelBrowserSession | null,
  queuedCount: number,
): StatusBlock {
  const health = session?.health_snapshot;
  if (health?.delivery?.state) {
    const state = String(health.delivery.state || "").trim();
    if (state === "failed") {
      return {
        label: "Activity",
        value: String(health.delivery.label || "").trim() || "Delivery failed",
        detail:
          String(health.delivery.reason || health.delivery.last_error || "").trim() ||
          "The last ChatGPT delivery did not complete.",
        tone: "error",
      };
    }
    if (state === "pending_bind") {
      return {
        label: "Activity",
        value: String(health.delivery.label || "").trim() || "Binding chat",
        detail:
          String(health.delivery.reason || "").trim() ||
          "Prompt was submitted; waiting for ChatGPT to assign the chat URL.",
        tone: "needs",
      };
    }
    if (state === "submitting") {
      return {
        label: "Activity",
        value: String(health.delivery.label || "").trim() || "Submitting",
        detail:
          String(health.delivery.reason || "").trim() ||
          "CCCC is injecting this batch into ChatGPT.",
        tone: "needs",
      };
    }
    if (state === "ambiguous") {
      return {
        label: "Activity",
        value: String(health.delivery.label || "").trim() || "Delivery unverified",
        detail:
          String(health.delivery.reason || health.delivery.last_error || "").trim() ||
          "CCCC attempted to submit the prompt, but could not verify whether ChatGPT accepted it.",
        tone: "needs",
      };
    }
    if (queuedCount > 0) {
      return {
        label: "Activity",
        value: `${queuedCount} queued`,
        detail: "Waiting for browser delivery.",
        tone: "needs",
      };
    }
    if ((state === "submitted" || state === "bound") && health.delivery.last_delivery_at) {
      const evidence = String(health.delivery.last_submission_evidence || "").trim();
      return {
        label: "Activity",
        value: `Last ${formatTime(health.delivery.last_delivery_at)}`,
        detail:
          state === "bound"
            ? String(health.delivery.reason || "").trim() || "ChatGPT chat binding completed."
            : evidence
              ? `Submitted: ${evidence}`
              : String(health.delivery.reason || "").trim() || "Browser delivery completed.",
        tone: "neutral",
      };
    }
  }
  const deliveryStatus = String(session?.last_delivery_status || "").trim();
  const lastError = String(session?.last_error || "").trim();
  if (deliveryStatus === "pending") {
    return {
      label: "Activity",
      value: "Binding chat",
      detail:
        lastError === "conversation_url_pending"
          ? "Prompt was submitted; waiting for ChatGPT to assign the chat URL."
          : lastError || "Prompt was submitted; waiting for ChatGPT to assign the chat URL.",
      tone: "needs",
    };
  }
  if (deliveryStatus === "submitting") {
    return {
      label: "Activity",
      value: "Submitting",
      detail: "CCCC is injecting this batch into ChatGPT.",
      tone: "needs",
    };
  }
  if (deliveryStatus === "ambiguous") {
    return {
      label: "Activity",
      value: "Delivery unverified",
      detail:
        lastError ||
        "CCCC attempted to submit the prompt, but could not verify whether ChatGPT accepted it.",
      tone: "needs",
    };
  }
  if (deliveryStatus === "failed" || lastError) {
    return {
      label: "Activity",
      value: "Delivery failed",
      detail: lastError || "The last ChatGPT delivery did not complete.",
      tone: "error",
    };
  }
  if (deliveryStatus === "bound") {
    return {
      label: "Activity",
      value: "Chat bound",
      detail: "ChatGPT chat binding completed.",
      tone: "neutral",
    };
  }
  if (queuedCount > 0) {
    return {
      label: "Activity",
      value: `${queuedCount} queued`,
      detail: "Waiting for browser delivery.",
      tone: "needs",
    };
  }
  if (session?.last_delivery_at) {
    const evidence = String(session.last_submission_evidence || "").trim();
    return {
      label: "Activity",
      value: `Last ${formatTime(session.last_delivery_at)}`,
      detail: evidence
        ? `Submitted: ${evidence}`
        : session.last_turn_id
          ? String(session.last_turn_id)
          : "Browser delivery completed.",
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

function shouldShowActivity(block: StatusBlock, queuedCount: number): boolean {
  if (queuedCount > 0) return true;
  if (block.tone !== "neutral") return true;
  return block.value !== "No recent delivery";
}

export function WebModelRuntimePanel({
  groupId,
  actor,
  isRunning,
  isDark,
  isVisible,
  readOnly,
}: WebModelRuntimePanelProps) {
  const { t } = useTranslation("chat");
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const [session, setSession] = useState<WebModelBrowserSession | null>(null);
  const [error, setError] = useState("");
  const [busyAction, setBusyAction] = useState("");
  const [surfaceRestartNonce, setSurfaceRestartNonce] = useState(0);
  const currentSelectionRef = useRef({ groupId, actorId: String(actor.id || "").trim() });
  const actorId = String(actor.id || "").trim();
  currentSelectionRef.current = { groupId, actorId };
  const queuedCount = Math.max(0, Number(actor.web_model_queued_count || 0));
  const canControlSurface = Boolean(isVisible && isRunning && !readOnly && groupId && actorId);

  useEffect(() => {
    if (!isVisible || !groupId || !actorId) {
      setSession(null);
      setError("");
      return;
    }
    let cancelled = false;
    setSession(null);
    setError("");
    setBusyAction("load");
    let loading = false;
    const load = (initial: boolean) => {
      if (loading) return;
      loading = true;
      void api
        .fetchWebModelBrowserSession(groupId, actorId, { inspect: false })
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
          loading = false;
          if (!cancelled && initial) setBusyAction("");
        });
    };
    load(true);
    const interval = window.setInterval(() => load(false), 2_000);
    return () => {
      window.clearInterval(interval);
      cancelled = true;
    };
  }, [actorId, groupId, isVisible]);

  const reloadChatGptPage = async () => {
    if (!groupId || !actorId) return;
    if (!canControlSurface) {
      const message = readOnly
        ? "ChatGPT page reload is disabled in read-only mode."
        : "Open ChatGPT Web Model settings to inspect the browser page.";
      setError(message);
      return;
    }
    setBusyAction("reload");
    setError("");
    try {
      const resp = await api.closeWebModelBrowserSurfaceSession(groupId, actorId);
      if (!matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId)) return;
      if (!resp.ok) {
        setError(resp.error?.message || "Failed to restart ChatGPT browser.");
        return;
      }
      setSession(resp.result.browser_session || {});
      setSurfaceRestartNonce((value) => value + 1);
    } finally {
      if (matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId))
        setBusyAction("");
    }
  };

  const openSettings = () => {
    openSettingsTarget({ scope: "global", tab: "webModels" });
  };

  const updateDeliveryMode = async (mode: WebModelDeliveryMode) => {
    if (!groupId || !actorId || readOnly || busyAction) return;
    if ((session?.delivery_mode || "standard") === mode) return;
    setBusyAction("delivery-mode");
    setError("");
    try {
      const resp = await api.updateWebModelDeliveryPreference({ groupId, actorId, mode });
      if (!matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId)) return;
      if (!resp.ok) {
        setError(resp.error?.message || t("webModelDelivery.modeSaveFailed"));
        return;
      }
      setSession(resp.result.browser_session || {});
    } catch {
      if (matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId))
        setError(t("webModelDelivery.modeSaveFailed"));
    } finally {
      if (matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId))
        setBusyAction("");
    }
  };

  const loadBrowserSurfaceSession = useCallback(async () => {
    const resp = await api.fetchWebModelBrowserSurfaceSession(groupId, actorId, { inspect: false });
    if (!matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId)) return resp;
    if (resp.ok) {
      setSession(resp.result.browser_session || {});
      setError("");
    } else {
      setError(resp.error?.message || "Failed to load ChatGPT browser surface.");
    }
    return resp;
  }, [actorId, groupId]);

  const startBrowserSurfaceSession = useCallback(
    async ({ width, height }: { width: number; height: number }) => {
      if (!canControlSurface) {
        const message = readOnly
          ? "ChatGPT browser control is disabled in read-only mode."
          : "Open ChatGPT Web Model settings to inspect the browser page.";
        setError(message);
        return {
          ok: false as const,
          error: { code: "browser_surface_unavailable", message, details: {} },
        };
      }
      const resp = await api.openWebModelBrowserSurfaceSession({
        groupId,
        actorId,
        width,
        height,
        inspect: true,
      });
      if (!matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId))
        return resp;
      if (resp.ok) {
        setSession(resp.result.browser_session || {});
        setError("");
      } else {
        setError(resp.error?.message || "Failed to open ChatGPT browser surface.");
      }
      return resp;
    },
    [actorId, canControlSurface, groupId, readOnly],
  );

  const chatGptBlock = useMemo(() => buildChatGptBlock(session), [session]);
  const targetBlock = useMemo(() => buildTargetBlock(session), [session]);
  const activityBlock = useMemo(
    () => buildActivityBlock(session, queuedCount),
    [queuedCount, session],
  );
  const primaryActionNeeded =
    !session?.ready || (!session?.conversation_url && !session?.pending_new_chat_bind);
  const nextAction = session?.health_snapshot?.next_action;
  const recommendedAction = String(nextAction?.recommended || "none").trim();
  const surfaceDisabledMessage = !isVisible
    ? ""
    : readOnly
      ? t("webModelDelivery.browserReadOnly")
      : !isRunning
        ? t("webModelDelivery.actorStoppedSurface")
        : "";
  const showActivity = shouldShowActivity(activityBlock, queuedCount);
  const nextSummary =
    recommendedAction && recommendedAction !== "none"
      ? String(nextAction?.label || "").trim() || recommendedAction
      : "";
  const deliveryMode: WebModelDeliveryMode =
    session?.delivery_mode === "image_compat" ? "image_compat" : "standard";
  const deliveryModeDisabled = Boolean(readOnly || busyAction);

  return (
    <section
      className={classNames(
        "flex min-h-0 flex-1 flex-col gap-3",
        isDark ? "text-slate-100" : "text-[rgb(35,36,37)]",
      )}
      aria-label="ChatGPT Web Model runtime"
    >
      <div
        className={classNames(
          "shrink-0 rounded-2xl border px-2 py-2 shadow-[0_14px_42px_-38px_rgba(15,23,42,0.65)]",
          isDark ? "border-white/10 bg-white/[0.035]" : "border-black/[0.07] bg-white/[0.78]",
        )}
      >
        <div className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex min-w-0 flex-wrap items-center gap-2 px-1">
            <span
              className={classNames(
                "shrink-0 rounded-full border px-2.5 py-1 text-[11px] font-semibold",
                tonePillClass(chatGptBlock.tone),
              )}
            >
              ChatGPT {chatGptBlock.value}
            </span>
            <span
              className={classNames(
                "shrink-0 rounded-full border px-2.5 py-1 text-[11px] font-semibold",
                tonePillClass(targetBlock.tone),
              )}
              title={targetBlock.detail}
            >
              Target {targetBlock.value}
            </span>
            {showActivity ? (
              <span
                className={classNames(
                  "shrink-0 rounded-full border px-2.5 py-1 text-[11px] font-semibold",
                  tonePillClass(activityBlock.tone),
                )}
                title={activityBlock.detail}
              >
                {activityBlock.value}
              </span>
            ) : null}
            {nextSummary ? (
              <span
                className="min-w-0 max-w-[min(54vw,520px)] truncate text-xs text-[var(--color-text-tertiary)]"
                title={nextAction?.reason ? `${nextSummary}: ${nextAction.reason}` : nextSummary}
              >
                Next: {nextSummary}
              </span>
            ) : null}
          </div>
          <div className="flex min-w-0 shrink-0 items-center justify-end gap-1.5">
            <fieldset
              className={classNames(
                "flex min-w-0 items-center",
                deliveryModeDisabled && "opacity-55",
              )}
              disabled={deliveryModeDisabled}
            >
              <legend className="sr-only">{t("webModelDelivery.modeTitle")}</legend>
              <span id="web-model-delivery-mode-scope" className="sr-only">
                {t("webModelDelivery.modeDescription")}
              </span>
              <div className="inline-flex h-10 min-w-0 items-center rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-1">
                {(
                  [
                    {
                      mode: "standard" as const,
                      label: t("webModelDelivery.modeStandard"),
                      detail: t("webModelDelivery.modeStandardDescription"),
                    },
                    {
                      mode: "image_compat" as const,
                      label: t("webModelDelivery.modeImageCompat"),
                      detail: t("webModelDelivery.modeImageCompatDescription"),
                    },
                  ] satisfies Array<{ mode: WebModelDeliveryMode; label: string; detail: string }>
                ).map((option) => {
                  const descriptionId = `web-model-delivery-mode-${option.mode}-description`;
                  return (
                    <HoverTooltip
                      key={option.mode}
                      label={<span className="block max-w-[220px] leading-4">{option.detail}</span>}
                    >
                      {(getReferenceProps, setReference) => (
                        <label
                          ref={setReference}
                          {...getReferenceProps({
                            className: classNames(
                              "relative min-w-0",
                              deliveryModeDisabled ? "cursor-not-allowed" : "cursor-pointer",
                            ),
                          })}
                        >
                          <input
                            type="radio"
                            name={`web-model-delivery-mode-${groupId}-${actorId}`}
                            value={option.mode}
                            checked={deliveryMode === option.mode}
                            onChange={() => void updateDeliveryMode(option.mode)}
                            aria-describedby={`${descriptionId} web-model-delivery-mode-scope`}
                            className="peer sr-only"
                          />
                          <span
                            className={classNames(
                              "inline-flex h-8 min-w-0 select-none items-center justify-center gap-1 rounded-lg border px-2.5 text-[11px] font-semibold transition-colors",
                              "peer-focus-visible:outline-none peer-focus-visible:ring-2 peer-focus-visible:ring-[rgb(143,163,187)]/55 peer-focus-visible:ring-offset-1",
                              deliveryMode === option.mode
                                ? "border-[var(--glass-tab-border-active)] bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] shadow-[var(--glass-tab-shadow-active)]"
                                : "border-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
                            )}
                          >
                            <span className="truncate">{option.label}</span>
                            {option.mode === "image_compat" ? (
                              <span className="shrink-0 rounded-full bg-amber-500/15 px-1 py-px text-[8px] font-bold uppercase leading-3 tracking-wide text-amber-700 dark:text-amber-300">
                                {t("webModelDelivery.modeImageCompatBadge")}
                              </span>
                            ) : null}
                          </span>
                          <span id={descriptionId} className="sr-only">
                            {option.detail}
                          </span>
                        </label>
                      )}
                    </HoverTooltip>
                  );
                })}
              </div>
              <span className="sr-only" aria-live="polite">
                {busyAction === "delivery-mode" ? t("webModelDelivery.modeSaving") : ""}
              </span>
            </fieldset>
            <Popover>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/45"
                  aria-label={t("webModelDelivery.modeHelp")}
                  title={t("webModelDelivery.modeHelp")}
                >
                  <InfoIcon size={15} aria-hidden="true" />
                </button>
              </PopoverTrigger>
              <PopoverContent
                align="end"
                className="w-[min(20rem,calc(100vw-1rem))] space-y-2 p-3 text-xs leading-5"
              >
                <div className="font-semibold text-[var(--color-text-primary)]">
                  {t("webModelDelivery.modeTitle")}
                </div>
                <p className="text-[var(--color-text-tertiary)]">
                  {t("webModelDelivery.modeDescription")}
                </p>
                <dl className="space-y-1.5 border-t border-[var(--glass-border-subtle)] pt-2">
                  <div>
                    <dt className="font-semibold text-[var(--color-text-secondary)]">
                      {t("webModelDelivery.modeStandard")}
                    </dt>
                    <dd className="text-[var(--color-text-tertiary)]">
                      {t("webModelDelivery.modeStandardDescription")}
                    </dd>
                  </div>
                  <div>
                    <dt className="font-semibold text-[var(--color-text-secondary)]">
                      {t("webModelDelivery.modeImageCompat")}
                    </dt>
                    <dd className="text-[var(--color-text-tertiary)]">
                      {t("webModelDelivery.modeImageCompatDescription")}
                    </dd>
                  </div>
                </dl>
              </PopoverContent>
            </Popover>
            <WebModelMcpShortcut
              groupId={groupId}
              actorId={actorId}
              actorRunning={isRunning}
              isVisible={isVisible}
              readOnly={readOnly}
              onOpenSettings={openSettings}
            />
            <button
              type="button"
              onClick={reloadChatGptPage}
              disabled={Boolean(busyAction) || !isRunning}
              className={iconButtonClass(false)}
              title="Restart ChatGPT browser"
              aria-label="Restart ChatGPT browser"
            >
              <RefreshIcon size={17} aria-hidden="true" />
            </button>
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
      </div>

      {canControlSurface ? (
        <div className="min-h-0 flex-1 overflow-hidden">
          <ProjectedBrowserSurfacePanel
            key={`chatgpt-runtime-surface:${groupId}:${actorId}:${surfaceRestartNonce}`}
            isDark={isDark}
            refreshNonce={0}
            defaultViewerMode="page"
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
        <div className="flex min-h-[240px] flex-1 items-center justify-center rounded-[18px] border border-dashed border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3 text-center text-xs leading-5 text-[var(--color-text-tertiary)]">
          {surfaceDisabledMessage}
        </div>
      ) : null}

      {error ? (
        <div
          role="alert"
          className="rounded-xl border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-xs leading-5 text-rose-700 dark:text-rose-300"
        >
          {error}
        </div>
      ) : null}
    </section>
  );
}
