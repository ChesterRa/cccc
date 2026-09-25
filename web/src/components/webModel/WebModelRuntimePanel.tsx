import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Actor } from "../../types";
import * as api from "../../services/api";
import type {
  WebModelBrowserSession,
  WebModelDeliveryMode,
  WebModelPairing,
} from "../../services/api";
import { classNames } from "../../utils/classNames";
import { formatTime } from "../../utils/time";
import { matchesWebModelActorSelection } from "../../utils/webModelSelection";
import { useModalStore } from "../../stores";
import { HoverTooltip } from "../HoverTooltip";
import { InfoIcon, RefreshIcon, SettingsIcon } from "../Icons";
import { ProjectedBrowserSurfacePanel } from "../browser/ProjectedBrowserSurfacePanel";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { WebModelConnectionStatus } from "./WebModelConnectionStatus";

interface WebModelRuntimePanelProps {
  groupId: string;
  actor: Actor;
  isRunning: boolean;
  isDark: boolean;
  isVisible: boolean;
  readOnly?: boolean;
}

function iconButtonClass(primary = false): string {
  return classNames(
    "inline-flex h-10 w-10 items-center justify-center rounded-xl border text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35 disabled:cursor-not-allowed disabled:opacity-50",
    primary
      ? "border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white hover:bg-black dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white/92"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
  );
}

export function WebModelRuntimePanel(props: WebModelRuntimePanelProps) {
  // A runtime change owns a different browser and must retire pending UI requests.
  return (
    <RuntimePanel key={`${props.groupId}/${props.actor.id}/${props.actor.runtime}`} {...props} />
  );
}

function RuntimePanel({
  groupId,
  actor,
  isRunning,
  isDark,
  isVisible,
  readOnly,
}: WebModelRuntimePanelProps) {
  const { t } = useTranslation("chat");
  const grok = actor.runtime === "grok_web_model";
  const openActorEditor = useModalStore((state) => state.openActorEditor);
  const [pairing, setPairing] = useState<WebModelPairing>();
  const [session, setSession] = useState<WebModelBrowserSession | null>(null);
  const [error, setError] = useState("");
  const [busyAction, setBusyAction] = useState("");
  const [surfaceRestartNonce, setSurfaceRestartNonce] = useState(0);
  const currentSelectionRef = useRef({ groupId, actorId: String(actor.id || "").trim() });
  const actorId = String(actor.id || "").trim();
  currentSelectionRef.current = { groupId, actorId };
  const queuedCount = Math.max(0, Number(actor.web_model_queued_count || 0));
  const canControlSurface = Boolean(
    isVisible && isRunning && !readOnly && groupId && actorId && (!grok || pairing?.url),
  );

  useEffect(() => {
    if (!isVisible || !groupId || !actorId) {
      setSession(null);
      setPairing(undefined);
      setError("");
      return;
    }
    let cancelled = false;
    setSession(null);
    setPairing(undefined);
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
            setError(resp.error?.message || t("webModelDelivery.statusFailed"));
            return;
          }
          setSession(resp.result.browser_session || {});
          setPairing(resp.result.pairing);
          setError("");
        })
        .catch(() => {
          if (!cancelled) setError(t("webModelDelivery.statusFailed"));
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
  }, [actorId, groupId, isVisible, t]);

  const reloadChatGptPage = async () => {
    if (!groupId || !actorId) return;
    if (!canControlSurface) {
      const message = readOnly
        ? t("webModelDelivery.browserReadOnly")
        : t("webModelDelivery.actorStoppedSurface");
      setError(message);
      return;
    }
    setBusyAction("reload");
    setError("");
    try {
      const resp = await api.reloadWebModelBrowserSession(groupId, actorId);
      if (!matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId)) return;
      if (!resp.ok) {
        setError(resp.error?.message || t("webModelDelivery.reloadFailed"));
        return;
      }
      setSession(resp.result.browser_session || {});
      setPairing(resp.result.pairing);
      setSurfaceRestartNonce((value) => value + 1);
    } catch {
      if (matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId))
        setError(t("webModelDelivery.reloadFailed"));
    } finally {
      if (matchesWebModelActorSelection(currentSelectionRef.current, groupId, actorId))
        setBusyAction("");
    }
  };

  const openSettings = () => {
    openActorEditor(actor, "chatgpt");
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
      setPairing(resp.result.pairing);
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
      setPairing(resp.result.pairing);
      setError("");
    } else {
      setError(resp.error?.message || t("webModelDelivery.statusFailed"));
    }
    return resp;
  }, [actorId, groupId, t]);

  const startBrowserSurfaceSession = useCallback(
    async ({ width, height }: { width: number; height: number }) => {
      if (!canControlSurface) {
        const message = readOnly
          ? t("webModelDelivery.browserReadOnly")
          : t("webModelDelivery.actorStoppedSurface");
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
        setPairing(resp.result.pairing);
        setError("");
      } else {
        setError(resp.error?.message || t("webModelDelivery.statusFailed"));
      }
      return resp;
    },
    [actorId, canControlSurface, groupId, readOnly, t],
  );

  const deliveryState =
    session?.health_snapshot?.delivery?.state || session?.last_delivery_status || "";
  const deliveryNeedsAttention = ["ambiguous", "blocked", "failed"].includes(deliveryState);
  const activity = deliveryNeedsAttention
    ? t(`webModelDelivery.activity.${deliveryState}`)
    : queuedCount > 0
      ? t("webModelDelivery.activity.queued", { count: queuedCount })
      : ["pending", "pending_bind", "submitting"].includes(deliveryState)
        ? t("webModelDelivery.activity.submitting")
        : session?.last_delivery_at
          ? t("webModelDelivery.activity.last", { time: formatTime(session.last_delivery_at) })
          : "";
  const surfaceDisabledMessage = !isVisible
    ? ""
    : readOnly
      ? t("webModelDelivery.browserReadOnly")
      : !isRunning
        ? t("webModelDelivery.actorStoppedSurface")
        : grok && !pairing?.url
          ? t(pairing ? "settings:grokActor.urlRequired" : "settings:webModelActor.states.loading")
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
      aria-label={grok ? "Grok Bot Web Model runtime" : "ChatGPT Web Model runtime"}
    >
      <div
        className={classNames(
          "shrink-0 rounded-2xl border px-2 py-2 shadow-[0_14px_42px_-38px_rgba(15,23,42,0.65)]",
          isDark ? "border-white/10 bg-white/[0.035]" : "border-black/[0.07] bg-white/[0.78]",
        )}
      >
        <div className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex min-w-0 flex-wrap items-center gap-2 px-1">
            <WebModelConnectionStatus
              pairing={pairing}
              session={session}
              provider={grok ? "grok_web" : "chatgpt_web"}
            />
            {activity && (
              <span
                className={classNames(
                  "text-xs",
                  deliveryNeedsAttention
                    ? "text-amber-700 dark:text-amber-300"
                    : "text-[var(--color-text-secondary)]",
                )}
              >
                {activity}
              </span>
            )}
          </div>
          <div className="flex min-w-0 shrink-0 flex-wrap items-center justify-end gap-1.5">
            {!grok && (
              <>
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
                      ] satisfies Array<{
                        mode: WebModelDeliveryMode;
                        label: string;
                        detail: string;
                      }>
                    ).map((option) => {
                      const descriptionId = `web-model-delivery-mode-${option.mode}-description`;
                      return (
                        <HoverTooltip
                          key={option.mode}
                          label={
                            <span className="block max-w-[220px] leading-4">{option.detail}</span>
                          }
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
              </>
            )}
            {!readOnly && (
              <button
                type="button"
                className="glass-btn px-3 py-2 inline-flex items-center gap-2"
                onClick={openSettings}
              >
                <SettingsIcon size={17} aria-hidden="true" />
                {t(grok ? "settings:grokActor.title" : "settings:webModelActor.title")}
              </button>
            )}
            <button
              type="button"
              onClick={reloadChatGptPage}
              disabled={Boolean(busyAction) || !canControlSurface}
              className={iconButtonClass(false)}
              title={t("webModelDelivery.refreshPage")}
              aria-label={t("webModelDelivery.refreshPage")}
            >
              <RefreshIcon size={17} aria-hidden="true" />
            </button>
          </div>
        </div>
      </div>

      {canControlSurface ? (
        <div className="min-h-0 flex-1 overflow-hidden">
          <ProjectedBrowserSurfacePanel
            key={`web-model-runtime-surface:${groupId}:${actorId}:${surfaceRestartNonce}`}
            sessionIdentity={`${groupId}/${actorId}/${actor.runtime}`}
            reuseActiveSession={false}
            isDark={isDark}
            refreshNonce={0}
            defaultViewerMode="page"
            chromeMode="embedded"
            viewportClassName="h-full min-h-0"
            loadSession={loadBrowserSurfaceSession}
            startSession={startBrowserSurfaceSession}
            webSocketUrl={api.getWebModelBrowserSurfaceWebSocketUrl(groupId, actorId)}
            fallbackUrl={grok ? pairing?.url : "https://chatgpt.com/"}
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
