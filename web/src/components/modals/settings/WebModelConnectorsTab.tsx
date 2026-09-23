import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { copyTextToClipboard } from "../../../utils/copy";
import { ProjectedBrowserSurfacePanel } from "../../browser/ProjectedBrowserSurfacePanel";
import {
  primaryButtonClass,
  secondaryButtonClass,
  dangerButtonClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspaceBodyClass,
  settingsWorkspacePanelClass,
} from "./types";

interface Props {
  isDark: boolean;
  isActive?: boolean;
  currentGroupId?: string;
  onOpenWebAccess?: () => void;
}
export default function WebModelConnectorsTab({ isDark, isActive = true, onOpenWebAccess }: Props) {
  const { t } = useTranslation("settings");
  const label = (key: string) => t(`webModelShared.${key}`);
  const [connector, setConnector] = useState<api.WebModelConnector | null>(null);
  const [browser, setBrowser] = useState<api.WebModelBrowserSession>({});
  const [needsSetup, setNeedsSetup] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [nonce, setNonce] = useState(0);
  const [confirm, setConfirm] = useState<"rotate" | "revoke" | null>(null);
  const [loadState, setLoadState] = useState<"loading" | "ready" | "error">("loading");
  const [loadNonce, setLoadNonce] = useState(0);
  const actionPending = useRef(false);
  useEffect(() => {
    // A credential returned by a pending mutation cannot be recovered by GET.
    if (!isActive || actionPending.current) return;
    let cancelled = false;
    setLoadState("loading");
    setError("");
    setConfirm(null);
    void Promise.all([api.fetchWebModelConnectors(), api.sharedWebModelBrowser()])
      .then(([c, b]) => {
        if (cancelled) return;
        if (c.ok) {
          setConnector((previous) => {
            const next = c.result.connectors.find((c) => !c.revoked) || null;
            return next && previous?.connector_id === next.connector_id
              ? { ...next, connector_url_path_token: previous?.connector_url_path_token }
              : next;
          });
          setNeedsSetup(Boolean(c.result.requires_reconfiguration));
          setLoadState("ready");
        } else {
          setLoadState("error");
          setError(c.error.message);
        }
        if (b.ok) setBrowser(b.result.browser_session);
        else setError(b.error.message);
      })
      .catch((e) => {
        if (cancelled) return;
        setLoadState("error");
        setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [isActive, loadNonce]);
  const loadBrowser = useCallback(() => api.sharedWebModelBrowser(), []);
  const actionsDisabled = busy || !isActive || loadState !== "ready";
  async function action(kind: "configure" | "revoke" | "open" | "check" | "close") {
    if (actionPending.current || actionsDisabled) return;
    actionPending.current = true;
    setBusy(true);
    setError("");
    setCopied(false);
    try {
      if (kind === "configure") {
        const r = await api.createWebModelConnector();
        if (!r.ok) throw new Error(r.error.message);
        setConnector(r.result.connector);
        setNeedsSetup(false);
        setConfirm(null);
      } else if (kind === "revoke" && connector) {
        const r = await api.revokeWebModelConnector(connector.connector_id);
        if (!r.ok) throw new Error(r.error.message);
        setConnector(null);
        setConfirm(null);
      } else {
        const r = await api.sharedWebModelBrowser(
          kind === "check" ? "status" : (kind as "open" | "close"),
          kind === "check",
        );
        if (!r.ok) throw new Error(r.error.message);
        setBrowser(r.result.browser_session);
        if (kind === "open") setNonce((n) => n + 1);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      actionPending.current = false;
      setBusy(false);
    }
  }
  const tokenUrl = connector?.connector_url_path_token || "";
  const url = tokenUrl ? new URL(tokenUrl, window.location.href).toString() : "";
  const publicHttps = (() => {
    try {
      const u = new URL(connector?.connector_url || "", window.location.href);
      return u.protocol === "https:" && !["localhost", "127.0.0.1", "[::1]"].includes(u.hostname);
    } catch {
      return false;
    }
  })();
  return (
    <div className={settingsWorkspaceShellClass(isDark)}>
      <div className={settingsWorkspaceHeaderClass(isDark)}>
        <div>
          <h2 className="text-lg font-semibold">{label("title")}</h2>
          <p className="mt-1 text-sm text-[var(--color-text-secondary)]">{label("description")}</p>
        </div>
      </div>
      <div className={settingsWorkspaceBodyClass}>
        {loadState === "loading" && <p role="status">{t("common:loading")}</p>}
        {error && (
          <p role="alert" className="text-red-600 dark:text-red-300 break-words">
            {error}
          </p>
        )}
        {loadState === "error" && (
          <button
            type="button"
            className={secondaryButtonClass()}
            disabled={!isActive}
            onClick={() => setLoadNonce((n) => n + 1)}
          >
            {t("common:retry")}
          </button>
        )}
        {needsSetup && (
          <p className="text-sm text-amber-700 dark:text-amber-300">{label("upgrade")}</p>
        )}
        <section className={settingsWorkspacePanelClass(isDark)}>
          <h3 className="font-semibold">{label("login")}</h3>
          <p className="mt-1 text-sm text-[var(--color-text-secondary)]">{label("loginHint")}</p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <span className="text-sm mr-auto">
              {loadState === "ready" &&
                label(
                  browser.verification_required
                    ? "verification"
                    : browser.ready
                      ? "ready"
                      : browser.active
                        ? "openState"
                        : "closed",
                )}
            </span>
            <button
              type="button"
              className={primaryButtonClass()}
              disabled={actionsDisabled}
              onClick={() => void action("open")}
            >
              {label("open")}
            </button>
            <button
              type="button"
              className={secondaryButtonClass()}
              disabled={actionsDisabled || !browser.active}
              onClick={() => void action("check")}
            >
              {label("check")}
            </button>
            <button
              type="button"
              className={secondaryButtonClass()}
              disabled={actionsDisabled || !browser.active}
              onClick={() => void action("close")}
            >
              {label("close")}
            </button>
          </div>
          {browser.active && isActive && (
            <div className="mt-4">
              <ProjectedBrowserSurfacePanel
                isDark={isDark}
                key={nonce}
                refreshNonce={0}
                viewportClassName="h-[min(70dvh,720px)] min-h-[320px] w-full"
                reuseActiveSession
                sessionIdentity="web-model-login"
                defaultViewerMode="browser"
                loadSession={loadBrowser}
                webSocketUrl={api.sharedWebModelBrowserWebSocketUrl()}
              />
            </div>
          )}
        </section>
        <section className={settingsWorkspacePanelClass(isDark)}>
          <h3 className="font-semibold">{label("connector")}</h3>
          <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
            {label("connectorHint")}
          </p>
          {loadState === "ready" && !publicHttps && (
            <div className="mt-3 text-sm">
              <p>{label("publicUrl")}</p>
              {onOpenWebAccess && (
                <button type="button" className={secondaryButtonClass()} onClick={onOpenWebAccess}>
                  {label("webAccess")}
                </button>
              )}
            </div>
          )}
          {connector && (
            <div className="mt-3 space-y-1 text-sm">
              <p>
                {label("configured")} · {connector.bound_actor_count || 0} {label("pairedActors")}
              </p>
              <p className="text-[var(--color-text-secondary)]">
                {connector.last_activity_at
                  ? t("webModelShared.lastSeen", {
                      time: new Date(connector.last_activity_at).toLocaleString(),
                    })
                  : label("notSeen")}
              </p>
            </div>
          )}
          {url && (
            <div className="mt-3 space-y-2">
              <p className="text-sm">{label("copyOnce")}</p>
              <button
                type="button"
                className={primaryButtonClass()}
                disabled={actionsDisabled}
                onClick={() =>
                  void copyTextToClipboard(url).then((ok) =>
                    ok ? setCopied(true) : setError(label("copyFailed")),
                  )
                }
              >
                {label(copied ? "copied" : "copy")}
              </button>
            </div>
          )}
          {!connector ? (
            <button
              type="button"
              className={`${secondaryButtonClass()} mt-3`}
              disabled={actionsDisabled}
              onClick={() => void action("configure")}
            >
              {label("create")}
            </button>
          ) : (
            <details
              className="mt-3 text-sm"
              onToggle={(event) => {
                if (!event.currentTarget.open) setConfirm(null);
              }}
            >
              <summary className="cursor-pointer text-[var(--color-text-secondary)]">
                {label("maintenance")}
              </summary>
              <div className="mt-3 flex flex-wrap gap-2">
                <button
                  type="button"
                  className={secondaryButtonClass()}
                  disabled={actionsDisabled}
                  onClick={() => setConfirm("rotate")}
                >
                  {label("rotate")}
                </button>
                {connector && (
                  <button
                    type="button"
                    className={dangerButtonClass()}
                    disabled={actionsDisabled}
                    onClick={() => setConfirm("revoke")}
                  >
                    {label("revoke")}
                  </button>
                )}
              </div>
              {confirm && (
                <div className="mt-3 border-t border-[var(--glass-border-subtle)] pt-3">
                  <p className="text-sm mb-2">
                    {label(confirm === "rotate" ? "rotateHint" : "revokeHint")}
                  </p>
                  <div className="flex gap-2">
                    <button
                      type="button"
                      className={dangerButtonClass()}
                      disabled={actionsDisabled}
                      onClick={() => void action(confirm === "rotate" ? "configure" : "revoke")}
                    >
                      {label("confirm")}
                    </button>
                    <button
                      type="button"
                      className={secondaryButtonClass()}
                      disabled={actionsDisabled}
                      onClick={() => setConfirm(null)}
                    >
                      {label("cancel")}
                    </button>
                  </div>
                </div>
              )}
            </details>
          )}
        </section>
        <p className="text-sm text-[var(--color-text-secondary)]">{label("actorHint")}</p>
      </div>
    </div>
  );
}
