import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { classNames } from "../../../utils/classNames";
import { copyTextToClipboard } from "../../../utils/copy";
import { ProjectedBrowserSurfacePanel } from "../../browser/ProjectedBrowserSurfacePanel";
import { SegmentedChoice } from "../../ui/segmented-choice";
import { connectorAddress } from "./connectorAddress";
import { ChatGPTPluginSetup } from "./ChatGPTPluginSetup";
import DotsSettingsTab from "./DotsSettingsTab";
import { WebModelConnectorDiagnostics } from "./WebModelConnectorDiagnostics";
import { SettingsCard, SettingsNotice, SettingsPage } from "./SettingsKit";
import { primaryButtonClass, secondaryButtonClass, dangerButtonClass } from "./types";

interface Props {
  provider?: api.WebModelView;
  onProviderChange?: (provider: api.WebModelView) => void;
  isDark: boolean;
  isActive?: boolean;
  currentGroupId?: string;
  onOpenWebAccess?: () => void;
  focusTarget?: { section: "login" | "connector"; nonce: number };
}
const PROVIDERS = [
  { value: "chatgpt_web" as const, label: "ChatGPT" },
  { value: "grok_web" as const, label: "Grok Bot" },
];
const VIEWS: Array<{ value: api.WebModelView; label: string }> = [
  ...PROVIDERS,
  { value: "dots", label: "Dots" },
];
export default function WebModelConnectorsTab(props: Props) {
  const [localProvider, setLocalProvider] = useState<api.WebModelView>("chatgpt_web");
  const [focusTarget, setFocusTarget] = useState(props.focusTarget);
  useEffect(() => setFocusTarget(props.focusTarget), [props.focusTarget]);
  const provider = props.provider ?? localProvider;
  const setProvider = props.onProviderChange ?? setLocalProvider;
  const openChatGPT = () => {
    setProvider("chatgpt_web");
    setFocusTarget({ section: "connector", nonce: Date.now() });
  };
  return (
    <div className="flex min-w-0 flex-col gap-5">
      <div>
        <SegmentedChoice
          value={provider}
          options={VIEWS}
          onChange={(value) => {
            setFocusTarget(undefined);
            setProvider(value);
          }}
          ariaLabel="Web Model"
          fill={false}
        />
      </div>
      {PROVIDERS.map(({ value }) => (
        <div key={value} hidden={provider !== value}>
          <ProviderSettings
            {...props}
            focusTarget={focusTarget}
            provider={value}
            isActive={props.isActive !== false && provider === value}
          />
        </div>
      ))}
      <div hidden={provider !== "dots"}>
        <DotsSettingsTab
          isActive={props.isActive !== false && provider === "dots"}
          onOpenChatGPT={openChatGPT}
          onOpenWebAccess={props.onOpenWebAccess}
        />
      </div>
    </div>
  );
}
/** One padded block inside a settings card. */
function CardBlock({ children }: { children: ReactNode }) {
  return <div className="min-w-0 space-y-3 px-5 py-3.5">{children}</div>;
}
function ProviderSettings({
  isDark,
  isActive = true,
  onOpenWebAccess,
  provider,
  focusTarget,
}: Omit<Props, "provider"> & { provider: api.WebModelProvider }) {
  const { t } = useTranslation("settings");
  const label = (key: string, options?: Record<string, string>) =>
    t(
      provider === "grok_web" &&
        [
          "title",
          "description",
          "connectorHint",
          "actorHint",
          "rotateHint",
          "revokeHint",
          "upgrade",
          "publicUrlMissing",
          "publicUrlInvalid",
          "copyOnce",
          "notSeen",
        ].includes(key)
        ? `grokShared.${key}`
        : `webModelShared.${key}`,
      options,
    );
  const [connector, setConnector] = useState<api.WebModelConnector | null>(null);
  // Undefined until read, or when an older Web server does not report it.
  const [connectorBase, setConnectorBase] = useState<string | undefined>();
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
  const loginRef = useRef<HTMLDivElement>(null);
  const connectorRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!isActive || !focusTarget) return;
    const frame = requestAnimationFrame(() => {
      const node = focusTarget.section === "login" ? loginRef.current : connectorRef.current;
      node?.scrollIntoView({ block: "start" });
      node?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [isActive, focusTarget]);
  useEffect(() => {
    // A credential returned by a pending mutation cannot be recovered by GET.
    if (!isActive || actionPending.current) return;
    let cancelled = false;
    setLoadState("loading");
    setError("");
    setConfirm(null);
    void Promise.all([
      api.fetchWebModelConnectors(),
      provider === "chatgpt_web"
        ? api.sharedWebModelBrowser("status", false, provider)
        : Promise.resolve(null),
    ])
      .then(([c, b]) => {
        if (cancelled) return;
        if (c.ok) {
          setConnector((previous) => {
            const next =
              c.result.connectors.find(
                (c) => !c.revoked && (c.provider || "chatgpt_web") === provider,
              ) || null;
            return next && previous?.connector_id === next.connector_id
              ? { ...next, connector_url_path_token: previous?.connector_url_path_token }
              : next;
          });
          setNeedsSetup(provider === "chatgpt_web" && Boolean(c.result.requires_reconfiguration));
          setConnectorBase(c.result.connector_base_url);
          setLoadState("ready");
        } else {
          setLoadState("error");
          setError(c.error.message);
        }
        if (b?.ok) setBrowser(b.result.browser_session);
        else if (b && !b.ok) setError(b.error.message);
      })
      .catch((e) => {
        if (cancelled) return;
        setLoadState("error");
        setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [isActive, loadNonce, provider]);
  const loadBrowser = useCallback(
    () => api.sharedWebModelBrowser("status", false, provider),
    [provider],
  );
  const actionsDisabled = busy || !isActive || loadState !== "ready";
  async function action(kind: "configure" | "revoke" | "open" | "check" | "close") {
    if (actionPending.current || actionsDisabled) return;
    if (kind === "configure" && address.kind !== "https") return;
    actionPending.current = true;
    setBusy(true);
    setError("");
    setCopied(false);
    try {
      if (kind === "configure") {
        const r = await api.createWebModelConnector(provider);
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
          provider,
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
  // An older Web server reports no base; judge its connector URL instead. An empty
  // base (no Web access address) still warns, and the page's own address never counts.
  const copyUrl = () =>
    void copyTextToClipboard(url).then((ok) =>
      ok ? setCopied(true) : setError(label("copyFailed")),
    );
  const address = connectorAddress(connectorBase ?? connector?.connector_url ?? "");
  const browserState = browser.verification_required
    ? "verification"
    : browser.ready
      ? "ready"
      : browser.login_required
        ? "loginRequired"
        : browser.state === "page_not_ready"
          ? "pageNotReady"
          : browser.active
            ? "openState"
            : "closed";
  return (
    <SettingsPage title={label("title")} description={label("description")}>
      {loadState === "loading" && (
        <p role="status" className="text-[13px] text-[var(--color-text-muted)]">
          {t("common:loading")}
        </p>
      )}
      {(error || loadState === "error") && (
        <SettingsNotice
          tone="error"
          action={
            loadState === "error" && (
              <button
                type="button"
                className={secondaryButtonClass()}
                disabled={!isActive}
                onClick={() => setLoadNonce((n) => n + 1)}
              >
                {t("common:retry")}
              </button>
            )
          }
        >
          {error}
        </SettingsNotice>
      )}
      {needsSetup && <SettingsNotice tone="warning">{label("upgrade")}</SettingsNotice>}
      {provider === "chatgpt_web" && (
        <div
          ref={loginRef}
          tabIndex={-1}
          className="scroll-mt-4 outline-none"
          data-web-model-section="login"
        >
          <SettingsCard
            title={label("login")}
            description={label("loginHint")}
            actions={
              <>
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
              </>
            }
          >
            {(loadState === "ready" || (browser.active && isActive)) && (
              <CardBlock>
                {loadState === "ready" && (
                  <span
                    className="inline-flex items-center gap-2 text-[13px] text-[var(--color-text-primary)]"
                    data-login-state={browserState}
                  >
                    <span
                      aria-hidden
                      className={classNames(
                        "size-2 shrink-0 rounded-full",
                        browserState === "ready"
                          ? "bg-emerald-500"
                          : browserState === "verification"
                            ? "bg-amber-500"
                            : browserState === "openState"
                              ? "bg-sky-500"
                              : "bg-[var(--color-text-muted)]",
                      )}
                    />
                    {label(browserState)}
                  </span>
                )}
                {browser.active && isActive && (
                  <ProjectedBrowserSurfacePanel
                    isDark={isDark}
                    key={nonce}
                    refreshNonce={0}
                    viewportClassName="h-[min(70dvh,720px)] min-h-[320px] w-full"
                    reuseActiveSession
                    sessionIdentity={`web-model-login:${provider}`}
                    defaultViewerMode="browser"
                    loadSession={loadBrowser}
                    webSocketUrl={api.sharedWebModelBrowserWebSocketUrl(provider)}
                  />
                )}
              </CardBlock>
            )}
          </SettingsCard>
        </div>
      )}
      <div
        ref={connectorRef}
        tabIndex={-1}
        className="scroll-mt-4 outline-none"
        data-web-model-section="connector"
      >
        <SettingsCard
          title={label("connector")}
          description={label("connectorHint")}
          actions={
            !connector && (
              <button
                type="button"
                className={primaryButtonClass()}
                disabled={actionsDisabled || address.kind !== "https"}
                onClick={() => void action("configure")}
              >
                {label("create")}
              </button>
            )
          }
        >
          {loadState === "ready" && address.kind !== "https" && (
            <CardBlock>
              <SettingsNotice
                tone="warning"
                action={
                  onOpenWebAccess && (
                    <button
                      type="button"
                      className={secondaryButtonClass()}
                      onClick={onOpenWebAccess}
                    >
                      {label("webAccess")}
                    </button>
                  )
                }
              >
                {address.kind === "missing"
                  ? label("publicUrlMissing")
                  : label("publicUrlInvalid", { url: address.origin })}
              </SettingsNotice>
            </CardBlock>
          )}
          {connector && (
            <CardBlock>
              <div className="space-y-1 text-[13px]">
                <p className="font-medium text-[var(--color-text-primary)]">
                  {label("configured")} · {connector.bound_actor_count || 0} {label("pairedActors")}
                </p>
                <p className="text-xs leading-5 text-[var(--color-text-muted)]">
                  {connector.last_activity_at
                    ? t("webModelShared.lastSeen", {
                        time: new Date(connector.last_activity_at).toLocaleString(),
                      })
                    : label("notSeen")}
                </p>
              </div>
            </CardBlock>
          )}
          {url && provider === "chatgpt_web" && (
            <CardBlock>
              <SettingsNotice tone="warning">{label("copyOnce")}</SettingsNotice>
              <ChatGPTPluginSetup
                disabled={actionsDisabled}
                urlCopied={copied}
                onCopyUrl={copyUrl}
                onCopyFailed={() => setError(label("copyFailed"))}
              />
            </CardBlock>
          )}
          {url && provider !== "chatgpt_web" && (
            <CardBlock>
              <SettingsNotice
                tone="warning"
                action={
                  <button
                    type="button"
                    className={primaryButtonClass()}
                    disabled={actionsDisabled}
                    onClick={copyUrl}
                  >
                    {label(copied ? "copied" : "copy")}
                  </button>
                }
              >
                {label("copyOnce")}
              </SettingsNotice>
            </CardBlock>
          )}
          {connector && (
            <CardBlock>
              <details
                className="text-[13px]"
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
                    disabled={actionsDisabled || address.kind !== "https"}
                    onClick={() => setConfirm("rotate")}
                  >
                    {label("rotate")}
                  </button>
                  <button
                    type="button"
                    className={dangerButtonClass()}
                    disabled={actionsDisabled}
                    onClick={() => setConfirm("revoke")}
                  >
                    {label("revoke")}
                  </button>
                </div>
                {confirm && (
                  <div className="mt-3">
                    <SettingsNotice
                      tone="warning"
                      action={
                        <>
                          <button
                            type="button"
                            className={dangerButtonClass()}
                            disabled={actionsDisabled}
                            onClick={() =>
                              void action(confirm === "rotate" ? "configure" : "revoke")
                            }
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
                        </>
                      }
                    >
                      {label(confirm === "rotate" ? "rotateHint" : "revokeHint")}
                    </SettingsNotice>
                  </div>
                )}
              </details>
            </CardBlock>
          )}
        </SettingsCard>
      </div>
      <SettingsNotice>{label("actorHint")}</SettingsNotice>
      {connector && (
        <WebModelConnectorDiagnostics
          connector={connector}
          disabled={actionsDisabled}
          onRefresh={() => setLoadNonce((value) => value + 1)}
        />
      )}
    </SettingsPage>
  );
}
