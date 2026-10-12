import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import type { DotsActorState } from "../../services/api";
import { Button } from "../ui/button";
import { formatMessageTimestamp } from "../../utils/time";

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
  setup = false,
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
    if (readOnly || !canAccessConnection || saving || action || !state) return;
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
  const locked = readOnly || saving || Boolean(action) || !state;
  const unresolved = Boolean(state?.active_turn_id);
  const canResolve = unresolved && (state?.phase === "failed" || state?.phase === "interrupted");
  const dotsLink = (
    <a
      href="https://chatgpt.com/dots/home"
      target="_blank"
      rel="noopener noreferrer"
      className="shrink-0 rounded-md px-2 py-1.5 text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2"
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

  return (
    <section className="min-h-0 overflow-y-auto text-sm" aria-label={t("dotsActor.title")}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] pb-3">
        <div className="min-w-0">
          <div className="font-semibold">{t("dotsActor.title")}</div>
          <p className="mt-1 text-[var(--color-text-secondary)]" role="status">
            {state
              ? t(`dotsActor.phases.${state.phase}`)
              : t(readError ? "dotsActor.unavailable" : "common:loading")}
            {state && state.pending_count > 0
              ? ` · ${t("dotsActor.pending", { count: state.pending_count })}`
              : ""}
          </p>
        </div>
        {dotsLink}
      </div>
      <p className="mt-3 text-xs leading-relaxed text-[var(--color-text-muted)]">
        {t("dotsActor.hint")}
      </p>
      {state?.last_execution ? (
        <p className="mt-3 text-xs text-[var(--color-text-secondary)]">
          {t(`dotsActor.lastExecution.${state.last_execution.status}`)}
          {state.last_execution.completed_at
            ? ` · ${formatMessageTimestamp(state.last_execution.completed_at)}`
            : ""}
        </p>
      ) : null}
      {state?.subscription_expires_at ? (
        <p className="mt-3 text-xs text-[var(--color-text-muted)]">
          {t(
            state.subscription_expires_at <= Date.now()
              ? "dotsActor.subscriptionExpired"
              : "dotsActor.subscriptionExpires",
            { time: new Date(state.subscription_expires_at).toLocaleString(i18n.language) },
          )}
        </p>
      ) : null}
      {state?.error_code ? (
        <p className="mt-3 text-xs text-rose-600 dark:text-rose-400">
          {t(deliveryErrorKeys[state.error_code] || "dotsActor.executionError")}
        </p>
      ) : null}
      {state?.phase === "failed" && !unresolved ? (
        <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("dotsActor.notifyAgain")}</p>
      ) : null}
      {readError ? (
        <p className="mt-3 text-xs text-amber-700 dark:text-amber-300" role="status">
          {readError}
        </p>
      ) : null}
      {actionError ? (
        <p className="mt-3 text-xs text-rose-600 dark:text-rose-400" role="alert">
          {actionError}
        </p>
      ) : null}
      {instructions ? (
        <div className="mt-4 space-y-2">
          <ol className="list-decimal space-y-1 pl-5 text-xs leading-relaxed text-[var(--color-text-secondary)]">
            <li>{t("dotsActor.nativeStepCopy")}</li>
            <li>{t("dotsActor.nativeStepPasteConfirm")}</li>
            <li>{t("dotsActor.nativeStepObserve")}</li>
          </ol>
          <label className="block text-xs font-medium" htmlFor={`dots-instructions-${actorId}`}>
            {t("dotsActor.instructions")}
          </label>
          <textarea
            id={`dots-instructions-${actorId}`}
            readOnly
            value={instructions}
            rows={5}
            className="w-full resize-y rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3 text-xs leading-relaxed"
          />
          <Button type="button" variant="outline" onClick={() => void copyInstructions()}>
            {t(copied ? "dotsActor.copied" : "dotsActor.copyInstructions")}
          </Button>
        </div>
      ) : null}
      {readOnly ? (
        !state?.subscribed ? (
          <p className="mt-4 text-xs text-[var(--color-text-secondary)]">
            {t("dotsActor.adminSetup")}
          </p>
        ) : null
      ) : (
        <div className="mt-4 space-y-3">
          {!state?.subscribed && !instructions ? (
            <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
              {t("dotsActor.connectHint")}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant={state?.subscribed ? "outline" : "default"}
              disabled={locked || unresolved}
              onClick={() => void update("connect")}
            >
              {t(state?.connected ? "dotsActor.getInstructions" : "dotsActor.connect")}
            </Button>
            {onOpenSharedSettings ? (
              <Button
                type="button"
                variant="outline"
                disabled={Boolean(action)}
                onClick={onOpenSharedSettings}
              >
                {t("dotsActor.sharedSettings")}
              </Button>
            ) : null}
          </div>
          {state?.connected ? (
            <details open={setup || undefined} className="text-xs text-[var(--color-text-muted)]">
              <summary className="cursor-pointer py-1.5">
                {t("dotsActor.connectionSettings")}
              </summary>
              {enabled ? <p className="my-2">{t("dotsActor.pauseBeforeChanging")}</p> : null}
              <Button
                className="mt-2"
                type="button"
                variant="outline"
                disabled={locked || Boolean(enabled) || unresolved}
                onClick={() => void update("disconnect")}
              >
                {t("dotsActor.disconnect")}
              </Button>
            </details>
          ) : null}
        </div>
      )}
      {canResolve && !readOnly ? (
        <div className="mt-4 space-y-2">
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
    </section>
  );
}
