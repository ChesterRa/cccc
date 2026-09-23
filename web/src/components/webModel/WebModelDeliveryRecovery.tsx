import { useCallback, useEffect, useId, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ProjectedBrowserSurfacePanel } from "../browser/ProjectedBrowserSurfacePanel";
import { ModalFrame } from "../modals/ModalFrame";
import { primaryButtonClass, secondaryButtonClass } from "../modals/settings/types";
import type { WebModelDeliveryStatus } from "../../utils/webModelDeliveryStatus";

type Scope = { groupId: string; actorId: string; actorLabel: string; deliveryId: string };

// Mount the observer only when opened, never one polling loop per message.
export function WebModelDeliveryRecovery({
  groupId,
  status,
  actorLabel,
  isDark,
  readOnly,
}: {
  groupId?: string;
  status: WebModelDeliveryStatus;
  actorLabel: string;
  isDark: boolean;
  readOnly?: boolean;
}) {
  const { t } = useTranslation("chat");
  const [scope, setScope] = useState<Scope | null>(null);
  const close = useCallback(() => setScope(null), []);
  if (readOnly) return null;
  return (
    <>
      {status.state === "ambiguous" && groupId && status.actorId && status.deliveryId ? (
        <button
          type="button"
          className="touch-target-sm rounded-lg px-2 py-1 text-xs font-semibold text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
          onClick={() =>
            setScope({
              groupId,
              actorId: status.actorId,
              actorLabel,
              deliveryId: status.deliveryId,
            })
          }
        >
          {t("webModelDelivery.recovery.open")}
        </button>
      ) : null}
      {scope
        ? createPortal(
            <RecoveryDialog scope={scope} isDark={isDark} onClose={close} />,
            document.body,
          )
        : null}
    </>
  );
}

function RecoveryDialog({
  scope,
  isDark,
  onClose,
}: {
  scope: Scope;
  isDark: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation(["chat", "common"]);
  const titleId = useId();
  const { modalRef } = useModalA11y(true, onClose);
  const [result, setResult] = useState<api.WebModelBrowserSurfaceResult | null>(null);
  const [loadError, setLoadError] = useState("");
  const [actionError, setActionError] = useState("");
  const [busy, setBusy] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const load = useCallback(
    () => api.fetchWebModelBrowserSurfaceSession(scope.groupId, scope.actorId, { inspect: false }),
    [scope.groupId, scope.actorId],
  );
  useEffect(() => {
    let cancelled = false,
      loading = false;
    const refresh = async () => {
      if (loading) return;
      loading = true;
      try {
        const response = await load();
        if (cancelled) return;
        if (response.ok) {
          setResult(response.result);
          setLoadError("");
        } else setLoadError(response.error.message);
      } catch (e) {
        if (!cancelled) setLoadError(String(e));
      } finally {
        loading = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [load]);
  const session = result?.browser_session;
  const sameDelivery = session?.last_delivery_id === scope.deliveryId;
  const resolved =
    acknowledged ||
    (sameDelivery &&
      ["submitted", "bound", "resolved"].includes(session?.last_delivery_status || ""));
  const canResume = sameDelivery && session?.can_resume_delivery && !resolved && !loadError;
  const resume = async () => {
    if (!canResume || busy) return;
    setBusy(true);
    setActionError("");
    try {
      const response = await api.resumeWebModelBrowserDelivery(
        scope.groupId,
        scope.actorId,
        scope.deliveryId,
      );
      if (!response.ok) setActionError(response.error.message);
      else {
        setAcknowledged(true);
        setResult(response.result);
      }
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const label = (key: string) => t(`webModelDelivery.recovery.${key}`);
  return (
    <ModalFrame
      isDark={isDark}
      surface="solid"
      onClose={onClose}
      titleId={titleId}
      title={t("webModelDelivery.recovery.title", { actor: scope.actorLabel })}
      closeAriaLabel={t("common:close")}
      modalRef={modalRef}
      panelClassName="w-full h-full sm:h-auto sm:max-h-[90dvh] sm:max-w-5xl"
      footerActions={
        <div className="flex flex-wrap justify-end gap-2">
          <button type="button" className={secondaryButtonClass()} onClick={onClose}>
            {t("common:close")}
          </button>
          <button
            type="button"
            className={primaryButtonClass()}
            disabled={!canResume || busy}
            onClick={() => void resume()}
          >
            {label(busy ? "resuming" : "resume")}
          </button>
        </div>
      }
    >
      <div className="min-h-0 flex-1 overflow-y-auto p-4 space-y-3 text-sm text-[var(--color-text-primary)]">
        <p>{label("instructions")}</p>
        <p className="text-[var(--color-text-secondary)]">{label("noReplay")}</p>
        <p role="status">
          {label(resolved ? "resolved" : result && !sameDelivery ? "changed" : "observing")}
        </p>
        {loadError || actionError ? (
          <p role="alert" className="break-words text-rose-700 dark:text-rose-300">
            {actionError || loadError}
          </p>
        ) : null}
        {result?.browser_surface.active ? (
          <ProjectedBrowserSurfacePanel
            isDark={isDark}
            refreshNonce={0}
            sessionIdentity={`${scope.groupId}/${scope.actorId}`}
            loadSession={load}
            webSocketUrl={api.getWebModelBrowserSurfaceWebSocketUrl(scope.groupId, scope.actorId)}
            viewportClassName="h-[45vh] min-h-[180px]"
          />
        ) : result ? (
          <p>{label("browserClosed")}</p>
        ) : null}
      </div>
    </ModalFrame>
  );
}
