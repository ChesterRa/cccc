import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import type { DotsActorState } from "../../services/api";
import { Button } from "../ui/button";
import { formatMessageTimestamp } from "../../utils/time";
import {
  DotsDeliverySteps,
  DotsSettingRow,
  DotsSetupStep,
  DotsSetupTips,
  DotsStatusSummary,
  dotsCard,
} from "./DotsActorSections";
import { DotsPluginStep } from "./DotsPluginStep";
import { phaseTone } from "./dotsActorStatus";

type DotsActorPanelProps = {
  groupId: string;
  actorId: string;
  isVisible: boolean;
  isRunning?: boolean;
  readOnly?: boolean;
  canAccessConnection?: boolean;
  setup?: boolean;
  saving?: boolean;
  onOpenSharedSettings?: () => void;
  onStateChange?: (state: DotsActorState | null) => void;
  onBusyChange?: (busy: boolean) => void;
};

const deliveryErrorKeys: Record<string, string> = {
  dots_callback_rejected: "dotsActor.errors.callbackRejected",
  dots_callback_failed: "dotsActor.errors.callbackFailed",
  dots_delivery_unconfirmed: "dotsActor.errors.deliveryUnconfirmed",
  dots_execution_interrupted: "dotsActor.errors.executionInterrupted",
  dots_subscription_gone: "dotsActor.errors.subscriptionGone",
};
const requestErrorKeys: Record<string, string> = {
  connector_unavailable: "dotsActor.errors.connectorRequired",
  dots_binding_conflict: "dotsActor.errors.bindingConflict",
  actor_must_be_stopped: "dotsActor.pauseBeforeChanging",
  delivery_unresolved: "dotsActor.reviewUnfinished",
  dots_state_unavailable: "dotsActor.readFailed",
};

export function DotsActorPanel(props: DotsActorPanelProps) {
  return <ActorPanel key={`${props.groupId}/${props.actorId}`} {...props} />;
}

function ActorPanel({
  groupId,
  actorId,
  isVisible,
  isRunning = false,
  readOnly = false,
  canAccessConnection = true,
  saving = false,
  onOpenSharedSettings,
  onStateChange,
  onBusyChange,
}: DotsActorPanelProps) {
  const { t, i18n } = useTranslation("settings");
  const [state, setState] = useState<DotsActorState | null>(null);
  const [readError, setReadError] = useState("");
  const [actionError, setActionError] = useState("");
  const [action, setAction] = useState("");
  const [pluginCreating, setPluginCreating] = useState(false);
  const [instructions, setInstructions] = useState("");
  const [copied, setCopied] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const revision = useRef(0);
  const visible = isVisible && canAccessConnection;
  const current = useRef({ isVisible: visible, onBusyChange, onStateChange, state, t });
  current.current = { isVisible: visible, onBusyChange, onStateChange, state, t };

  useEffect(() => {
    if (!visible) {
      revision.current += 1;
      setInstructions("");
      setAction("");
      current.current.onBusyChange?.(false);
      return;
    }
    let cancelled = false;
    let loading = false;
    const load = () => {
      if (loading || action) return;
      loading = true;
      const startedRevision = revision.current;
      void api
        .fetchDotsActorState(groupId, actorId)
        .then((response) => {
          if (cancelled || !current.current.isVisible || startedRevision !== revision.current)
            return;
          if (!response.ok) {
            const key = requestErrorKeys[response.error?.code || ""];
            setReadError(
              key
                ? current.current.t(key)
                : response.error?.message || current.current.t("dotsActor.readFailed"),
            );
            if (!current.current.state) current.current.onStateChange?.(null);
            return;
          }
          if (response.result.group_id !== groupId || response.result.actor_id !== actorId) return;
          setState(response.result);
          setReadError("");
          current.current.onStateChange?.(response.result);
        })
        .catch(() => {
          if (!cancelled && current.current.isVisible && startedRevision === revision.current) {
            setReadError(current.current.t("dotsActor.readFailed"));
            if (!current.current.state) current.current.onStateChange?.(null);
          }
        })
        .finally(() => {
          loading = false;
        });
    };
    load();
    const timer = window.setInterval(load, 2_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [groupId, actorId, visible, isRunning, refresh, action]);

  useEffect(
    () => () => {
      current.current.isVisible = false;
      current.current.onBusyChange?.(false);
    },
    [],
  );

  const update = async (nextAction: "connect" | "disconnect" | "resolve") => {
    if (readOnly || !canAccessConnection || saving || action || pluginCreating || !state) return;
    if (nextAction === "disconnect" && (state.enabled || isRunning)) return;
    const turnId = nextAction === "resolve" ? state.active_turn_id || undefined : undefined;
    if (nextAction !== "resolve" && state.active_turn_id) return;
    if (nextAction === "resolve" && (!turnId || !window.confirm(t("dotsActor.resolveConfirm"))))
      return;
    if (nextAction === "disconnect" && !window.confirm(t("dotsActor.disconnectConfirm"))) return;
    const startedRevision = ++revision.current;
    setAction(nextAction);
    setActionError("");
    setCopied(false);
    current.current.onBusyChange?.(true);
    try {
      const response = await api.updateDotsActorConnection(groupId, actorId, nextAction, turnId);
      if (!current.current.isVisible || startedRevision !== revision.current) return;
      if (!response.ok) {
        const key = requestErrorKeys[response.error?.code || ""];
        setActionError(key ? t(key) : response.error?.message || t("dotsActor.actionFailed"));
        return;
      }
      setInstructions(nextAction === "connect" ? response.result.instructions || "" : "");
      setRefresh((value) => value + 1);
    } catch {
      if (current.current.isVisible && startedRevision === revision.current)
        setActionError(t("dotsActor.actionFailed"));
    } finally {
      if (current.current.isVisible && startedRevision === revision.current) {
        setAction("");
        current.current.onBusyChange?.(false);
      }
    }
  };

  const copyInstructions = async () => {
    const startedRevision = revision.current;
    try {
      await navigator.clipboard.writeText(instructions);
      if (current.current.isVisible && startedRevision === revision.current) setCopied(true);
    } catch {
      if (current.current.isVisible && startedRevision === revision.current)
        setActionError(t("dotsActor.copyFailed"));
    }
  };
  const enabled = isRunning || state?.enabled;
  const locked = readOnly || saving || Boolean(action) || pluginCreating || !state;
  const unresolved = Boolean(state?.active_turn_id);
  const hasDelivery = Boolean((state?.paired && state.subscribed) || unresolved);
  const canResolve = unresolved && (state?.phase === "failed" || state?.phase === "interrupted");
  const dotsLink = (
    <a
      href="https://chatgpt.com/dots/home"
      target="_blank"
      rel="noopener noreferrer"
      className="inline-flex h-9 shrink-0 items-center rounded-lg border border-[var(--glass-border-subtle)] px-3 text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2"
    >
      {t("dotsActor.openDots")}
    </a>
  );
  if (!canAccessConnection)
    return (
      <section className="min-h-0 overflow-y-auto text-sm" aria-label={t("dotsActor.title")}>
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] pb-3">
          <div>
            <div className="font-semibold">{t("dotsActor.title")}</div>
            <p className="mt-1 text-[var(--color-text-secondary)]" role="status">
              {t("dotsActor.adminStatus")}
            </p>
          </div>
          {dotsLink}
        </div>
        <p className="mt-3 text-xs leading-relaxed text-[var(--color-text-secondary)]">
          {t("dotsActor.adminManaged")}
        </p>
      </section>
    );

  const subscription = !state
    ? t("dotsActor.unavailable")
    : state.subscription_expires_at
      ? t(
          state.subscription_expires_at <= Date.now()
            ? "dotsActor.subscriptionExpired"
            : "dotsActor.subscriptionExpires",
          { time: new Date(state.subscription_expires_at).toLocaleString(i18n.language) },
        )
      : t(state.subscribed ? "dotsActor.status.permanent" : "dotsActor.status.notSubscribed");
  const lastTask = !state
    ? t("dotsActor.unavailable")
    : state.last_execution
      ? `${t(`dotsActor.lastExecution.${state.last_execution.status}`)}${
          state.last_execution.completed_at
            ? ` · ${formatMessageTimestamp(state.last_execution.completed_at)}`
            : ""
        }`
      : t("dotsActor.status.noTask");
  const connectButton = (
    <Button
      type="button"
      variant={state?.subscribed || instructions ? "outline" : "default"}
      disabled={locked || unresolved}
      onClick={() => void update("connect")}
    >
      {t(state?.connected ? "dotsActor.getInstructions" : "dotsActor.connect")}
    </Button>
  );
  const sharedButton = onOpenSharedSettings ? (
    <Button
      type="button"
      variant="outline"
      disabled={Boolean(action) || pluginCreating}
      onClick={onOpenSharedSettings}
    >
      {t("dotsActor.sharedSettings")}
    </Button>
  ) : null;
  const disconnectRow =
    !readOnly && state?.connected ? (
      <DotsSettingRow
        label={t("dotsActor.disconnect")}
        hint={t(enabled ? "dotsActor.pauseBeforeChanging" : "dotsActor.settings.disconnectHint")}
        danger
      >
        <Button
          type="button"
          variant="outline"
          disabled={locked || Boolean(enabled) || unresolved}
          onClick={() => void update("disconnect")}
        >
          {t("dotsActor.disconnect")}
        </Button>
      </DotsSettingRow>
    ) : null;
  const instructionsBox = instructions ? (
    <>
      <label className="text-xs font-medium" htmlFor={`dots-instructions-${actorId}`}>
        {t("dotsActor.instructions")}
      </label>
      <textarea
        id={`dots-instructions-${actorId}`}
        readOnly
        value={instructions}
        rows={5}
        className="min-h-32 w-full flex-1 resize-y rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3 font-mono text-xs leading-relaxed"
      />
      <div>
        <Button type="button" disabled={locked} onClick={() => void copyInstructions()}>
          {t(copied ? "dotsActor.copied" : "dotsActor.copyInstructions")}
        </Button>
      </div>
      <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
        {t("dotsActor.nativeStepPasteConfirm")}
      </p>
    </>
  ) : null;

  return (
    <section
      className="@container/dots flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm"
      aria-label={t("dotsActor.title")}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-base font-semibold">{t("dotsActor.title")}</h2>
            <p
              className={`inline-flex h-6 items-center rounded-full px-2.5 text-xs font-semibold ${
                state ? phaseTone(state.phase) : phaseTone("unconfigured")
              }`}
              role="status"
            >
              {state
                ? t(`dotsActor.phases.${state.phase}`)
                : t(readError ? "dotsActor.unavailable" : "common:loading")}
              {state && state.pending_count > 0
                ? ` · ${t("dotsActor.pending", { count: state.pending_count })}`
                : ""}
            </p>
          </div>
          <p className="mt-1 text-xs leading-relaxed text-[var(--color-text-muted)]">
            {t("dotsActor.hint")}
          </p>
        </div>
        {dotsLink}
      </div>
      {state?.error_code ? (
        <p className="text-xs text-rose-600 dark:text-rose-400">
          {t(deliveryErrorKeys[state.error_code] || "dotsActor.executionError")}
        </p>
      ) : null}
      {state?.phase === "failed" && !unresolved ? (
        <p className="text-xs text-[var(--color-text-muted)]">{t("dotsActor.notifyAgain")}</p>
      ) : null}
      {readError ? (
        <p className="text-xs text-amber-700 dark:text-amber-300" role="status">
          {readError}
        </p>
      ) : null}
      {actionError ? (
        <p className="text-xs text-rose-600 dark:text-rose-400" role="alert">
          {actionError}
        </p>
      ) : null}
      <DotsStatusSummary state={state} t={t} subscription={subscription} lastTask={lastTask} />
      {canResolve && !readOnly ? (
        <div className={`${dotsCard} flex flex-wrap items-center justify-between gap-3 px-4 py-3`}>
          <p className="text-xs text-[var(--color-text-muted)]">
            {t("dotsActor.reviewUnfinished")}
          </p>
          <Button
            type="button"
            variant="outline"
            disabled={locked}
            onClick={() => void update("resolve")}
          >
            {t("dotsActor.resolve")}
          </Button>
        </div>
      ) : null}
      {readOnly && state && !hasDelivery ? (
        <p className="text-xs text-[var(--color-text-secondary)]">{t("dotsActor.adminSetup")}</p>
      ) : hasDelivery || readOnly ? (
        <div className="grid flex-1 gap-4 @3xl/dots:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
          {state ? <DotsDeliverySteps state={state} t={t} /> : null}
          {readOnly ? null : (
            <section
              className={`${dotsCard} flex flex-col`}
              aria-label={t("dotsActor.connectionSettings")}
            >
              <h3 className="px-4 py-3 font-semibold">{t("dotsActor.connectionSettings")}</h3>
              {instructions ? (
                <div className="flex flex-col gap-2 border-t border-[var(--glass-border-subtle)] px-4 py-3">
                  {instructionsBox}
                </div>
              ) : null}
              <DotsSettingRow
                label={t("dotsActor.settings.instructions")}
                hint={t("dotsActor.settings.instructionsHint")}
              >
                {connectButton}
              </DotsSettingRow>
              {sharedButton ? (
                <DotsSettingRow
                  label={t("dotsActor.settings.shared")}
                  hint={t("dotsActor.settings.sharedHint")}
                >
                  {sharedButton}
                </DotsSettingRow>
              ) : null}
              <div className="flex-1" />
              {disconnectRow}
            </section>
          )}
        </div>
      ) : (
        <div className="grid flex-1 gap-4 @3xl/dots:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
          <ol className={`${dotsCard} flex flex-col overflow-hidden`}>
            <DotsSetupStep
              number={1}
              title={t("dotsActor.setup.pluginTitle")}
              hint={t("dotsActor.connectHint")}
            >
              <DotsPluginStep
                isVisible={visible}
                disabled={locked}
                sharedSettings={sharedButton}
                onBusyChange={(busy) => {
                  setPluginCreating(busy);
                  current.current.onBusyChange?.(busy);
                }}
              />
            </DotsSetupStep>
            <DotsSetupStep
              number={2}
              title={t("dotsActor.setup.copyTitle")}
              hint={t("dotsActor.nativeStepCopy")}
              active
            >
              {instructionsBox}
              <div>{connectButton}</div>
            </DotsSetupStep>
            <DotsSetupStep
              number={3}
              title={t("dotsActor.setup.waitTitle")}
              hint={t("dotsActor.nativeStepObserve")}
            />
          </ol>
          <DotsSetupTips t={t}>
            {disconnectRow ? (
              <div className={`${dotsCard} overflow-hidden [&>div]:border-t-0`}>
                {disconnectRow}
              </div>
            ) : null}
          </DotsSetupTips>
        </div>
      )}
    </section>
  );
}
