import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import type { DotsActorState } from "../../services/api";
import { Button } from "../ui/button";
import { formatMessageTimestamp } from "../../utils/time";
import { DotsSettingRow, DotsSetupStep, DotsSetupTips } from "./DotsActorSections";
import {
  NativeActorDelivery,
  NativeActorSummary,
  nativeActorCard as dotsCard,
} from "./NativeActorSections";
import { DotsPluginStep } from "./DotsPluginStep";
import { phaseTone } from "./nativeActorStatus";

type DotsActorPanelProps = {
  groupId: string;
  actorId: string;
  isVisible: boolean;
  isRunning?: boolean;
  readOnly?: boolean;
  canAccessConnection?: boolean;
  setup?: boolean;
  saving?: boolean;
  compact?: boolean;
  onExpand?: () => void;
  onSettings?: () => void;
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
  delivery_unresolved: "nativeActor.connectionBusy",
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
  compact = false,
  setup = false,
  onExpand,
  onSettings,
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
    const turnId = nextAction === "resolve" ? state.active_turn_id || undefined : undefined;
    if (nextAction !== "resolve" && state.active_turn_id) return;
    if (
      nextAction === "resolve" &&
      (!turnId ||
        !window.confirm(t("nativeActor.endTurnConfirm", { provider: t("dotsActor.title") })))
    )
      return;
    const pauseFirst = nextAction === "disconnect" && state.enabled;
    if (
      nextAction === "disconnect" &&
      !window.confirm(
        t(pauseFirst ? "dotsActor.pauseDisconnectConfirm" : "dotsActor.disconnectConfirm"),
      )
    )
      return;
    const startedRevision = ++revision.current;
    setAction(nextAction);
    setActionError("");
    setCopied(false);
    current.current.onBusyChange?.(true);
    try {
      if (pauseFirst) {
        const stopped = await api.stopActor(groupId, actorId);
        if (!stopped.ok) throw new Error(stopped.error.message);
      }
      const response = await api.updateDotsActorConnection(groupId, actorId, nextAction, turnId);
      if (!current.current.isVisible || startedRevision !== revision.current) return;
      if (!response.ok) {
        const key = requestErrorKeys[response.error?.code || ""];
        setActionError(key ? t(key) : response.error?.message || t("dotsActor.actionFailed"));
        return;
      }
      setInstructions(nextAction === "connect" ? response.result.instructions || "" : "");
      setRefresh((value) => value + 1);
    } catch (error) {
      if (current.current.isVisible && startedRevision === revision.current)
        setActionError(error instanceof Error ? error.message : t("dotsActor.actionFailed"));
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
  const enabled = state?.enabled ?? isRunning;
  const locked = readOnly || saving || Boolean(action) || pluginCreating || !state;
  const unresolved = Boolean(state?.active_turn_id);
  const hasDelivery = Boolean((state?.paired && state.subscribed) || unresolved);
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
  const summaries: Array<[string, string]> = [
    [t("dotsActor.status.subscription"), subscription],
    [
      t("dotsActor.status.pending"),
      state ? t("dotsActor.pending", { count: state.pending_count }) : t("dotsActor.unavailable"),
    ],
    [t("dotsActor.status.lastTask"), lastTask],
  ];
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
          disabled={locked || unresolved}
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

  const notices = (
    <>
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
    </>
  );
  const recovery =
    unresolved && !readOnly ? (
      <div className="flex flex-col items-start gap-2 @lg/nativeActor:flex-row @lg/nativeActor:items-center @lg/nativeActor:justify-between @lg/nativeActor:gap-3">
        <p className="min-w-0 flex-1 text-xs leading-relaxed text-[var(--color-text-muted)]">
          {t("nativeActor.endTurnHint", { provider: t("dotsActor.title") })}
        </p>
        <Button
          type="button"
          variant="outline"
          className="shrink-0"
          disabled={locked}
          onClick={() => void update("resolve")}
        >
          {t("nativeActor.endTurn")}
        </Button>
      </div>
    ) : null;
  if (compact && onExpand)
    return (
      <section
        className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-2 text-sm"
        aria-label={t("dotsActor.title")}
      >
        <div className="flex flex-wrap items-center gap-2">
          <p
            role="status"
            className={`rounded-full px-2 py-1 text-xs font-semibold ${phaseTone(state?.phase || "unconfigured")}`}
          >
            {state
              ? t(`dotsActor.phases.${state.phase}`)
              : t(readError ? "dotsActor.unavailable" : "common:loading")}
          </p>
          <Button type="button" variant="outline" className="ml-auto" onClick={onExpand}>
            {t("dotsActor.openDetails")}
          </Button>
          {dotsLink}
        </div>
        {notices}
        <NativeActorSummary cells={summaries} compact />
        {hasDelivery && state ? (
          <NativeActorDelivery phase={state.phase} provider="dots" t={t} compact>
            {recovery}
          </NativeActorDelivery>
        ) : (
          <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
            {t(readOnly ? "dotsActor.adminSetup" : "dotsActor.nativeStepObserve")}
          </p>
        )}
      </section>
    );

  return (
    <section
      className="@container/nativeActor flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm"
      aria-label={t("dotsActor.title")}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-base font-semibold">{t("dotsActor.title")}</h2>
            {!setup && (
              <p
                className={`inline-flex min-h-6 items-center rounded-full px-2.5 py-1 text-xs font-semibold ${
                  state ? phaseTone(state.phase) : phaseTone("unconfigured")
                }`}
                role="status"
              >
                {state
                  ? t(`dotsActor.phases.${state.phase}`)
                  : t(readError ? "dotsActor.unavailable" : "common:loading")}
              </p>
            )}
          </div>
          {!setup && (
            <p className="mt-1 text-xs leading-relaxed text-[var(--color-text-muted)]">
              {t("dotsActor.hint")}
            </p>
          )}
          {setup && (
            <p
              role="status"
              className="mt-1 text-xs leading-relaxed text-[var(--color-text-muted)]"
            >
              {t("dotsActor.status.pairing")}:{" "}
              {state
                ? t(state.paired ? "dotsActor.status.paired" : "dotsActor.status.notPaired")
                : t("dotsActor.unavailable")}
              {" · "}
              {t("dotsActor.status.subscription")}: {subscription}
            </p>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          {!setup &&
            !readOnly &&
            onSettings &&
            (compact ||
              (state &&
                ["unconfigured", "waiting_pairing", "waiting_subscription"].includes(
                  state.phase,
                ))) && (
              <Button variant="outline" onClick={onSettings}>
                {t("dotsActor.connectionSettings")}
              </Button>
            )}
          {dotsLink}
        </div>
      </div>
      {notices}
      {setup && state?.paired && !enabled && !unresolved && (
        <p role="status" className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
          {t("dotsActor.pairedPaused")}
        </p>
      )}
      {!setup && <NativeActorSummary cells={summaries} />}
      {setup && recovery}
      {setup && unresolved && (
        <p role="status" className="text-xs text-[var(--color-text-muted)]">
          {t("nativeActor.connectionBusy")}
        </p>
      )}
      {!setup && state && hasDelivery && (
        <NativeActorDelivery phase={state.phase} provider="dots" t={t}>
          {recovery}
        </NativeActorDelivery>
      )}
      {readOnly && state && !hasDelivery ? (
        <p className="text-xs text-[var(--color-text-secondary)]">{t("dotsActor.adminSetup")}</p>
      ) : readOnly || (!setup && hasDelivery) ? null : hasDelivery ? (
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
          {disconnectRow}
        </section>
      ) : (
        <div className="grid shrink-0 items-start gap-4 @3xl/nativeActor:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
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
