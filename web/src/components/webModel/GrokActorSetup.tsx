import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import { ProjectedBrowserSurfacePanel } from "../browser/ProjectedBrowserSurfacePanel";
import { secondaryButtonClass, inputClass } from "../modals/settings/types";
import { WebModelConnectionStatus } from "./WebModelConnectionStatus";

export function GrokActorSetup({
  groupId,
  actorId,
  isDark,
  isVisible = true,
  onOpenSharedSettings,
  draftUrl,
  onDraftChange,
  onEnabledChange,
  onBusyChange,
  saving = false,
}: {
  groupId: string;
  actorId: string;
  isDark: boolean;
  isVisible?: boolean;
  onOpenSharedSettings?: () => void;
  draftUrl?: string;
  onDraftChange: (url: string | undefined) => void;
  onEnabledChange?: (enabled: boolean) => void;
  onBusyChange?: (busy: boolean) => void;
  saving?: boolean;
}) {
  const { t } = useTranslation("settings");
  const [result, setResult] = useState<api.WebModelBrowserSurfaceResult>();
  const [editing, setEditing] = useState(false);
  const [viewing, setViewing] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [error, setError] = useState("");
  const [acting, setBusy] = useState(false);
  const busy = acting || saving;
  const revision = useRef(0);
  const pending = useRef(false);
  const alive = useRef(true);
  const fetchStatus = useCallback(
    () => api.fetchWebModelBrowserSurfaceSession(groupId, actorId),
    [groupId, actorId],
  );
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (!isVisible) return;
    let cancelled = false,
      loading = false;
    const load = async () => {
      if (pending.current || loading) return;
      loading = true;
      const own = revision.current;
      try {
        const response = await fetchStatus();
        if (cancelled || own !== revision.current) return;
        if (response.ok) setResult(response.result);
        else setError(response.error.message);
      } catch (e) {
        if (!cancelled && own === revision.current) setError(String(e));
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
  const enabled = pairing?.actor_enabled !== false;
  const saved = pairing?.url || "";
  useEffect(() => {
    if (pairing?.actor_enabled !== undefined) onEnabledChange?.(pairing.actor_enabled);
  }, [pairing?.actor_enabled, onEnabledChange]);
  async function action(kind: "open" | "resume" | "remove") {
    if (busy || pending.current || !isVisible) return;
    pending.current = true;
    revision.current++;
    setBusy(true);
    onBusyChange?.(true);
    setError("");
    try {
      if (kind === "remove" && enabled) {
        const stopped = await api.stopActor(groupId, actorId);
        if (!stopped.ok) throw new Error(stopped.error.message);
      }
      const response =
        kind === "open"
          ? await api.openWebModelBrowserSurfaceSession({ groupId, actorId })
          : kind === "remove"
            ? await api.changeWebModelPairing(groupId, actorId, "remove")
            : await api.resumeWebModelBrowserDelivery(
                groupId,
                actorId,
                result?.browser_session.last_delivery_id || "",
              );
      if (!alive.current) return;
      if (!response.ok) throw new Error(response.error.message);
      const fresh = await fetchStatus();
      if (!alive.current) return;
      if (!fresh.ok) throw new Error(fresh.error.message);
      setResult(fresh.result);
      setConfirmRemove(false);
      if (kind === "remove") {
        setViewing(false);
        setEditing(false);
        onDraftChange(undefined);
      }
      if (kind === "open") setViewing(true);
    } catch (e) {
      if (alive.current) setError(e instanceof Error ? e.message : String(e));
    } finally {
      pending.current = false;
      revision.current++;
      if (alive.current) {
        setBusy(false);
        onBusyChange?.(false);
      }
    }
  }
  return (
    <section className="min-w-0 space-y-4 rounded-2xl border border-[var(--glass-border-subtle)] p-4 sm:p-5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="font-semibold">{t("grokActor.title")}</h3>
        <WebModelConnectionStatus
          provider="grok_web"
          pairing={pairing}
          session={result?.browser_session}
        />
      </div>
      <p className="text-sm text-[var(--color-text-secondary)]">{t("grokActor.hint")}</p>
      {onOpenSharedSettings && (
        <button type="button" className={secondaryButtonClass()} onClick={onOpenSharedSettings}>
          {t("grokActor.shared")}
        </button>
      )}
      {saved && <p className="break-all text-sm">{saved}</p>}
      {error && (
        <p role="alert" className="break-words text-red-600 dark:text-red-300">
          {error}
        </p>
      )}
      {!saved || editing || draftUrl !== undefined ? (
        <div className="space-y-2">
          <label className="block text-sm">
            {t("grokActor.url")}
            <input
              type="url"
              required
              value={draftUrl ?? saved}
              onChange={(e) =>
                onDraftChange(e.target.value.trim() === saved ? undefined : e.target.value)
              }
              className={`${inputClass(isDark)} mt-2 w-full`}
              placeholder="https://grok.com/bot/…"
              disabled={busy || !result}
            />
          </label>
          <p className="text-sm text-[var(--color-text-secondary)]">{t("grokActor.draftHint")}</p>
          {saved && (
            <button
              type="button"
              className={`${secondaryButtonClass()} ml-2`}
              disabled={busy}
              onClick={() => {
                setEditing(false);
                onDraftChange(undefined);
              }}
            >
              {t("common:cancel")}
            </button>
          )}
        </div>
      ) : null}
      <div className="flex flex-wrap gap-2">
        {saved && !editing && draftUrl === undefined && (
          <button
            type="button"
            className={secondaryButtonClass()}
            disabled={busy}
            onClick={() => {
              setEditing(true);
            }}
          >
            {t("grokActor.change")}
          </button>
        )}
        {saved && (
          <button
            type="button"
            className={secondaryButtonClass()}
            disabled={busy}
            onClick={() => void action("open")}
          >
            {t("grokActor.open")}
          </button>
        )}
      </div>
      {saved && (
        <div className="text-sm">
          {confirmRemove ? (
            <>
              <p>{t(enabled ? "webModelActor.pauseRemoveConfirm" : "webModelActor.removeHint")}</p>
              <button
                type="button"
                className={secondaryButtonClass()}
                disabled={busy}
                onClick={() => void action("remove")}
              >
                {t("grokActor.confirmRemove")}
              </button>
              <button
                type="button"
                className={secondaryButtonClass()}
                disabled={busy}
                onClick={() => setConfirmRemove(false)}
              >
                {t("common:cancel")}
              </button>
            </>
          ) : (
            <button
              type="button"
              className={secondaryButtonClass()}
              disabled={busy}
              onClick={() => setConfirmRemove(true)}
            >
              {t("grokActor.remove")}
            </button>
          )}
        </div>
      )}
      {viewing && saved && isVisible && (
        <ProjectedBrowserSurfacePanel
          isDark={isDark}
          refreshNonce={0}
          sessionIdentity={`${groupId}/${actorId}`}
          defaultViewerMode="page"
          viewportClassName="h-[min(70dvh,720px)] min-h-[320px] w-full"
          reuseActiveSession
          loadSession={fetchStatus}
          startSession={({ width, height }) =>
            api.openWebModelBrowserSurfaceSession({ groupId, actorId, width, height })
          }
          webSocketUrl={api.getWebModelBrowserSurfaceWebSocketUrl(groupId, actorId)}
          fallbackUrl={saved}
        />
      )}
      {result?.browser_session.can_resume_delivery && (
        <button
          type="button"
          className={secondaryButtonClass()}
          disabled={busy}
          onClick={() => void action("resume")}
        >
          {t("webModelActor.resume")}
        </button>
      )}
    </section>
  );
}
