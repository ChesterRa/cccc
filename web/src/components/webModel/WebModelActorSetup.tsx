import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import { ProjectedBrowserSurfacePanel } from "../browser/ProjectedBrowserSurfacePanel";
import { primaryButtonClass, secondaryButtonClass, inputClass } from "../modals/settings/types";
import { WebModelConnectionStatus } from "./WebModelConnectionStatus";
import { webModelConnectionState } from "./webModelConnectionState";

export function WebModelActorSetup({
  groupId,
  actorId,
  isDark,
  isVisible = true,
  onOpenSharedSettings,
}: {
  groupId: string;
  actorId: string;
  isDark: boolean;
  isVisible?: boolean;
  onOpenSharedSettings?: () => void;
}) {
  const { t } = useTranslation("settings");
  const label = (key: string) => t(`webModelActor.${key}`);
  const [result, setResult] = useState<api.WebModelBrowserSurfaceResult | null>(null);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [nonce, setNonce] = useState(0);
  const [viewing, setViewing] = useState(false);
  const [changing, setChanging] = useState(false);
  const [attemptId, setAttemptId] = useState("");
  const [confirmRemove, setConfirmRemove] = useState(false);
  const mutation = useRef(0);
  const selection = useRef("");
  selection.current = `${groupId}/${actorId}`;
  const fetchStatus = useCallback(
    () => api.fetchWebModelBrowserSurfaceSession(groupId, actorId),
    [groupId, actorId],
  );
  useEffect(() => {
    setResult(null);
    setUrl("");
    setError("");
    setConfirmRemove(false);
    setViewing(false);
    setChanging(false);
    setAttemptId("");
  }, [groupId, actorId]);
  useEffect(() => {
    if (!isVisible) return;
    let cancelled = false,
      loading = false;
    const load = async () => {
      if (loading || mutation.current % 2 !== 0) return;
      loading = true;
      const revision = mutation.current;
      try {
        const r = await fetchStatus();
        if (cancelled || revision !== mutation.current) return;
        if (r.ok) setResult(r.result);
        else setError(r.error.message);
      } catch (e) {
        if (!cancelled && revision === mutation.current) setError(String(e));
      } finally {
        loading = false;
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [fetchStatus, isVisible]);
  const pairing = result?.pairing;
  const enabled = pairing?.actor_enabled === true;
  const { state, bound, connecting, failed } = webModelConnectionState(
    pairing,
    result?.browser_session,
  );
  const restoring = !bound && !!pairing?.previous_url;
  // Only the completed replacement attempt closes this flow; the old binding stays valid meanwhile.
  useEffect(() => {
    if (attemptId && pairing?.pairing_id === attemptId && state === "bound") {
      setChanging(false);
      setAttemptId("");
      setUrl("");
    }
  }, [attemptId, pairing?.pairing_id, state]);
  async function action(
    kind: "stop" | "open" | "navigate" | "new" | "connect" | "cancel" | "remove" | "resume",
    value?: string,
  ) {
    if (busy) return;
    const identity = selection.current;
    mutation.current += 1;
    setBusy(true);
    setError("");
    try {
      const r =
        kind === "stop"
          ? await api.stopActor(groupId, actorId)
          : kind === "open"
            ? await api.openWebModelBrowserSurfaceSession({ groupId, actorId })
            : kind === "navigate" || kind === "new"
              ? await api.bindCurrentWebModelBrowserConversation({
                  groupId,
                  actorId,
                  conversationUrl: url,
                  newChat: kind === "new",
                })
              : kind === "resume"
                ? await api.resumeWebModelBrowserDelivery(groupId, actorId, value || "")
                : await api.changeWebModelPairing(groupId, actorId, kind, pairing?.pairing_id);
      if (selection.current !== identity) return;
      if (!r.ok)
        throw new Error(
          t(`webModelActor.errors.${r.error.code}`, { defaultValue: r.error.message }),
        );
      setConfirmRemove(false);
      if (kind === "connect") setAttemptId((r.result as api.WebModelPairing).pairing_id || "");
      if (kind === "open" || kind === "navigate" || kind === "new") {
        setViewing(true);
        setNonce((n) => n + 1);
      }
      if (kind === "remove") setChanging(false);
      const next = await fetchStatus();
      if (selection.current === identity) {
        if (next.ok) setResult(next.result);
        else setError(next.error.message);
      }
    } catch (e) {
      if (selection.current === identity) setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (selection.current === identity) {
        mutation.current += 1;
        setBusy(false);
      }
    }
  }
  const configuring = !bound || changing;
  const showBrowser = viewing || connecting;
  return (
    <section className="min-w-0 space-y-4 rounded-xl border border-[var(--glass-border-subtle)] p-4">
      <div className="flex flex-wrap items-start gap-3">
        <div className="mr-auto min-w-0">
          <h3 className="font-semibold">{label("title")}</h3>
          <WebModelConnectionStatus pairing={pairing} session={result?.browser_session} />
        </div>
        {onOpenSharedSettings ? (
          <button type="button" className={secondaryButtonClass()} onClick={onOpenSharedSettings}>
            {label("sharedSettings")}
          </button>
        ) : (
          <p className="text-sm text-[var(--color-text-secondary)]">
            {label("sharedSettingsHint")}
          </p>
        )}
      </div>
      {error && (
        <p role="alert" className="break-words text-sm text-red-600 dark:text-red-300">
          {error}
        </p>
      )}
      {bound && (
        <p className="text-sm break-all">
          {label("pairedUrl")}: {pairing?.url}
        </p>
      )}
      {!bound && pairing?.previous_url && (
        <p className="text-sm break-all">
          {label("previousUrl")}: {pairing.previous_url}
        </p>
      )}
      {state === "bound" && !changing && (
        <p className="text-sm text-[var(--color-text-secondary)]">{label("done")}</p>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          className={secondaryButtonClass()}
          disabled={busy || connecting}
          onClick={() => {
            if (viewing) setViewing(false);
            else if (result?.browser_session.active) setViewing(true);
            else void action("open");
          }}
        >
          {label(viewing ? "hideBrowser" : "open")}
        </button>
        {bound && !changing && !connecting && (
          <button
            type="button"
            className={secondaryButtonClass()}
            disabled={busy}
            onClick={() => {
              setChanging(true);
              if (result?.browser_session.active) setViewing(true);
            }}
          >
            {label("changeConversation")}
          </button>
        )}
      </div>
      {configuring && (
        <div className="space-y-3 border-t border-[var(--glass-border-subtle)] pt-3">
          <p className="text-sm text-[var(--color-text-secondary)]">
            {label(bound ? "changeHint" : restoring ? "restoreHint" : "pairHint")}
          </p>
          {enabled && !connecting && (bound || failed || restoring) && (
            <div className="flex flex-wrap items-center gap-2">
              <p className="text-sm mr-auto">{label("stopHint")}</p>
              <button
                type="button"
                className={secondaryButtonClass()}
                disabled={busy}
                onClick={() => void action("stop")}
              >
                {label("stop")}
              </button>
            </div>
          )}
          {!enabled && (
            <>
              <button
                type="button"
                className={secondaryButtonClass()}
                disabled={busy || connecting}
                onClick={() => void action("new")}
              >
                {label("new")}
              </button>
              <div>
                <label htmlFor={`web-model-url-${actorId}`} className="block text-sm mb-2">
                  {label("url")}
                </label>
                <div className="flex flex-wrap gap-2">
                  <input
                    id={`web-model-url-${actorId}`}
                    className={`${inputClass(isDark)} min-w-0 flex-1`}
                    value={url}
                    onChange={(e) => setUrl(e.target.value)}
                    placeholder="https://chatgpt.com/c/…"
                    disabled={busy || connecting}
                  />
                  <button
                    type="button"
                    className={secondaryButtonClass()}
                    disabled={busy || connecting || !url.trim()}
                    onClick={() => void action("navigate")}
                  >
                    {label("openUrl")}
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      )}
      {isVisible && showBrowser && result?.browser_session.active && (
        <ProjectedBrowserSurfacePanel
          isDark={isDark}
          key={nonce}
          refreshNonce={0}
          viewportClassName="h-[min(70dvh,720px)] min-h-[320px] w-full"
          sessionIdentity={`actor-${groupId}-${actorId}`}
          reuseActiveSession
          defaultViewerMode="page"
          loadSession={fetchStatus}
          webSocketUrl={api.getWebModelBrowserSurfaceWebSocketUrl(groupId, actorId)}
        />
      )}
      <div className="flex flex-wrap gap-2">
        {!connecting && (((changing || restoring) && !enabled) || (failed && !bound)) && (
          <button
            type="button"
            className={primaryButtonClass()}
            disabled={busy || state === "connector_required" || !result?.browser_session.active}
            onClick={() => void action("connect")}
          >
            {label(changing || restoring ? "useConversation" : "retry")}
          </button>
        )}
        {connecting && (
          <button
            type="button"
            className={secondaryButtonClass()}
            disabled={busy}
            onClick={() => void action("cancel")}
          >
            {label("cancel")}
          </button>
        )}
        {changing && !connecting && (
          <button
            type="button"
            className={secondaryButtonClass()}
            disabled={busy}
            onClick={() => {
              setChanging(false);
              setViewing(false);
              setConfirmRemove(false);
            }}
          >
            {label("closeChange")}
          </button>
        )}
      </div>
      {changing && !enabled && !connecting && (
        <details className="text-sm">
          <summary className="cursor-pointer text-[var(--color-text-secondary)]">
            {label("maintenance")}
          </summary>
          <div className="mt-3 space-y-2">
            <p>{label("removeHint")}</p>
            <button
              type="button"
              className={secondaryButtonClass()}
              disabled={busy}
              onClick={() => (confirmRemove ? void action("remove") : setConfirmRemove(true))}
            >
              {label(confirmRemove ? "confirmRemove" : "remove")}
            </button>
            {confirmRemove && (
              <button
                type="button"
                className={secondaryButtonClass()}
                onClick={() => setConfirmRemove(false)}
              >
                {t("webModelShared.cancel")}
              </button>
            )}
          </div>
        </details>
      )}
      {result?.browser_session.last_error && (
        <p className="text-sm break-words" role="status">
          {result.browser_session.last_error}
        </p>
      )}
      {result?.browser_session.can_resume_delivery && (
        <button
          type="button"
          className={secondaryButtonClass()}
          disabled={busy}
          onClick={() => void action("resume", result.browser_session.last_delivery_id)}
        >
          {label("resume")}
        </button>
      )}
    </section>
  );
}
