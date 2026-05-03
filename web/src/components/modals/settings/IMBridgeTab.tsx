// IMBridgeTab configures IM bridge settings.
import { useState, useEffect, useCallback } from "react";
import { useTranslation, Trans } from "react-i18next";
import QRCode from "qrcode";
import { IMStatus, IMPlatform, WeixinLoginStatus } from "../../../types";
import * as api from "../../../services/api";
import { SelectCombobox } from "../../SelectCombobox";
import {
  dangerButtonClass,
  inputClass,
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";
import { copyTextToClipboard } from "../../../utils/copy";

const IM_PENDING_AUTO_REFRESH_MS = 12000;

interface IMBridgeTabProps {
  isDark: boolean;
  groupId?: string; // Reserved for future use.
  imStatus: IMStatus | null;
  imPlatform: IMPlatform;
  onPlatformChange: (v: IMPlatform) => void;
  imBotTokenEnv: string;
  setImBotTokenEnv: (v: string) => void;
  imAppTokenEnv: string;
  setImAppTokenEnv: (v: string) => void;
  // Feishu fields
  imFeishuDomain: string;
  setImFeishuDomain: (v: string) => void;
  imFeishuAppId: string;
  setImFeishuAppId: (v: string) => void;
  imFeishuAppSecret: string;
  setImFeishuAppSecret: (v: string) => void;
  // DingTalk fields
  imDingtalkAppKey: string;
  setImDingtalkAppKey: (v: string) => void;
  imDingtalkAppSecret: string;
  setImDingtalkAppSecret: (v: string) => void;
  imDingtalkRobotCode: string;
  setImDingtalkRobotCode: (v: string) => void;
  // WeCom fields
  imWecomBotId: string;
  setImWecomBotId: (v: string) => void;
  imWecomSecret: string;
  setImWecomSecret: (v: string) => void;
  // Weixin fields
  imWeixinAccountId: string;
  setImWeixinAccountId: (v: string) => void;
  weixinLoginStatus: WeixinLoginStatus | null;
  onStartWeixinLogin: () => void;
  onLogoutWeixin: () => void;
  // Actions
  imBusy: boolean;
  onSaveConfig: () => void;
  onRemoveConfig: () => void;
  onStartBridge: () => void;
  onStopBridge: () => void;
}

function WeixinQrCode({
  value,
  loadingLabel,
  errorLabel,
}: {
  value: string;
  loadingLabel: string;
  errorLabel: string;
}) {
  const [dataUrl, setDataUrl] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    const content = String(value || "").trim();
    if (!content) return;

    void QRCode.toDataURL(content, {
      errorCorrectionLevel: "M",
      margin: 2,
      width: 320,
      color: {
        dark: "#334155",
        light: "#ffffff",
      },
    })
      .then((nextUrl) => {
        if (cancelled) return;
        setDataUrl(nextUrl);
        setError("");
      })
      .catch((nextError: unknown) => {
        if (cancelled) return;
        setDataUrl("");
        setError(nextError instanceof Error ? nextError.message : String(nextError));
      });

    return () => {
      cancelled = true;
    };
  }, [value]);

  if (error) {
    return (
      <div className="mt-3 rounded-lg border border-red-500/20 bg-red-500/5 p-3 text-xs text-red-500">
        {errorLabel}: {error}
      </div>
    );
  }

  if (!dataUrl) {
    return (
      <div className="mt-3 flex h-56 w-56 items-center justify-center rounded-lg border border-black/10 bg-white p-2 text-xs text-[var(--color-text-muted)]">
        {loadingLabel}
      </div>
    );
  }

  return (
    <img
      src={dataUrl}
      alt="Weixin QR Code"
      className="h-56 w-56 rounded-lg border border-black/10 bg-white p-2"
    />
  );
}

export function IMBridgeTab({
  isDark: _isDark,
  groupId,
  imStatus,
  imPlatform,
  onPlatformChange,
  imBotTokenEnv,
  setImBotTokenEnv,
  imAppTokenEnv,
  setImAppTokenEnv,
  imFeishuDomain,
  setImFeishuDomain,
  imFeishuAppId,
  setImFeishuAppId,
  imFeishuAppSecret,
  setImFeishuAppSecret,
  imDingtalkAppKey,
  setImDingtalkAppKey,
  imDingtalkAppSecret,
  setImDingtalkAppSecret,
  imDingtalkRobotCode,
  setImDingtalkRobotCode,
  imWecomBotId,
  setImWecomBotId,
  imWecomSecret,
  setImWecomSecret,
  imWeixinAccountId,
  setImWeixinAccountId,
  weixinLoginStatus,
  onStartWeixinLogin,
  onLogoutWeixin,
  imBusy,
  onSaveConfig,
  onRemoveConfig,
  onStartBridge,
  onStopBridge,
}: IMBridgeTabProps) {
  const { t } = useTranslation("settings");
  const [weixinQrCopyState, setWeixinQrCopyState] = useState<"idle" | "done" | "failed">("idle");
  const sectionTitleClass = "text-sm font-semibold text-[var(--color-text-primary)]";
  const sectionHintClass = "mt-1 text-xs text-[var(--color-text-tertiary)]";
  const compactSecondaryButtonClass = secondaryButtonClass("sm");
  const compactDangerButtonClass = dangerButtonClass("sm");
  const weixinStatus = String(weixinLoginStatus?.status || "").trim().toLowerCase();
  const weixinErrorText = String(weixinLoginStatus?.error || "").trim();
  const weixinLoggedIn = !!weixinLoginStatus?.logged_in;
  const weixinHasQr = !!String(weixinLoginStatus?.qrcode_url || "").trim();
  const weixinHasCustomAdvanced = !!String(imWeixinAccountId || "").trim();
  const getBotTokenLabel = () => {
    switch (imPlatform) {
      case "telegram": return t("imBridge.botTokenTelegram");
      case "slack": return t("imBridge.botTokenSlack");
      case "discord": return t("imBridge.botTokenDiscord");
      default: return t("imBridge.botToken");
    }
  };

  const getBotTokenPlaceholder = () => {
    switch (imPlatform) {
      case "telegram": return "TELEGRAM_BOT_TOKEN (or 123456:ABC...)";
      case "slack": return "SLACK_BOT_TOKEN (or xoxb-...)";
      case "discord": return "DISCORD_BOT_TOKEN (or <token>)";
      default: return "";
    }
  };

  const canSaveIM = () => {
    if (imPlatform === "feishu") {
      return !!imFeishuAppId && !!imFeishuAppSecret;
    }
    if (imPlatform === "dingtalk") {
      return !!imDingtalkAppKey && !!imDingtalkAppSecret;
    }
    if (imPlatform === "wecom") {
      return !!imWecomBotId && !!imWecomSecret;
    }
    if (imPlatform === "weixin") {
      return true;
    }
    if (!imBotTokenEnv) return false;
    if (imPlatform === "slack" && !imAppTokenEnv) return false;
    return true;
  };

  const needsBotToken = imPlatform === "telegram" || imPlatform === "slack" || imPlatform === "discord";

  // Authorized chats state
  const [authChats, setAuthChats] = useState<api.IMAuthorizedChat[]>([]);
  const [authLoading, setAuthLoading] = useState(false);
  const [authError, setAuthError] = useState("");
  const [authInfo, setAuthInfo] = useState("");
  const [revoking, setRevoking] = useState<string | null>(null);
  const [pendingRequests, setPendingRequests] = useState<api.IMPendingRequest[]>([]);
  const [pendingLoading, setPendingLoading] = useState(false);
  const [pendingError, setPendingError] = useState("");
  const [pendingActionKey, setPendingActionKey] = useState<string | null>(null);
  const [showBindInput, setShowBindInput] = useState(false);
  const [bindKey, setBindKey] = useState("");
  const [binding, setBinding] = useState(false);
  const weixinAuthorizedChatCount = authChats.filter(
    (chat) => String(chat.platform || "").trim().toLowerCase() === "weixin",
  ).length;

  const loadAuthorizedChats = useCallback(async () => {
    if (!groupId) return;
    setAuthLoading(true);
    setAuthError("");
    try {
      const resp = await api.fetchIMAuthorized(groupId);
      if (resp.ok) {
        setAuthChats(resp.result?.authorized ?? []);
      } else {
        setAuthError(resp.error?.message || t("imBridge.loadAuthorizedFailed"));
      }
    } catch {
      setAuthError(t("imBridge.loadAuthorizedFailed"));
    } finally {
      setAuthLoading(false);
    }
  }, [groupId, t]);

  const loadPendingRequests = useCallback(async (opts?: { silent?: boolean }) => {
    if (!groupId) return;
    const silent = !!opts?.silent;
    if (!silent) {
      setPendingLoading(true);
      setPendingError("");
    }
    try {
      const resp = await api.fetchIMPending(groupId);
      if (resp.ok) {
        setPendingRequests(resp.result?.pending ?? []);
      } else {
        if (!silent) {
          setPendingError(resp.error?.message || t("imBridge.loadPendingFailed"));
        }
      }
    } catch {
      if (!silent) {
        setPendingError(t("imBridge.loadPendingFailed"));
      }
    } finally {
      if (!silent) {
        setPendingLoading(false);
      }
    }
  }, [groupId, t]);

  const onCopyWeixinQrLink = useCallback(async () => {
    const link = String(weixinLoginStatus?.qrcode_url || "").trim();
    if (!link) {
      setWeixinQrCopyState("failed");
      return;
    }
    const ok = await copyTextToClipboard(link);
    if (ok) {
      setWeixinQrCopyState("done");
    } else {
      setWeixinQrCopyState("failed");
    }
  }, [weixinLoginStatus?.qrcode_url]);

  useEffect(() => {
    if (weixinQrCopyState === "idle") return;
    const timer = window.setTimeout(() => setWeixinQrCopyState("idle"), 1800);
    return () => window.clearTimeout(timer);
  }, [weixinQrCopyState]);

  const getWeixinStatusLabel = () => {
    if (weixinLoggedIn) return t("imBridge.weixinStatusLoggedIn");
    if (weixinStatus === "waiting_scan") return t("imBridge.weixinStatusWaitingScan");
    if (weixinStatus === "starting_login") return t("imBridge.weixinStatusStarting");
    if (weixinStatus === "error") return t("imBridge.weixinStatusError");
    return t("imBridge.weixinNotLoggedIn");
  };

  const getWeixinHint = () => {
    if (weixinHasQr || weixinStatus === "waiting_scan") return t("imBridge.weixinHintWaitingScan");
    if (weixinLoggedIn) return t("imBridge.weixinHintLoggedIn");
    if (weixinStatus === "error") return t("imBridge.weixinHintError");
    return t("imBridge.weixinHintIdle");
  };

  const getWeixinSubscribeBodyKey = () => {
    if (weixinAuthorizedChatCount > 0) return "imBridge.weixinSubscribeBoundBody";
    if (!imStatus?.configured) return "imBridge.weixinSubscribeNeedsConfigBody";
    if (!imStatus.running) return "imBridge.weixinSubscribeNeedsRunningBody";
    return "imBridge.weixinSubscribeNextBody";
  };

  const loadIMAuthState = useCallback(async () => {
    await Promise.all([loadAuthorizedChats(), loadPendingRequests()]);
  }, [loadAuthorizedChats, loadPendingRequests]);

  useEffect(() => {
    if (imStatus?.configured) {
      loadIMAuthState();
    }
  }, [imStatus?.configured, loadIMAuthState]);

  useEffect(() => {
    if (!imStatus?.configured) return;
    const timer = window.setInterval(() => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        return;
      }
      void loadPendingRequests({ silent: true });
    }, IM_PENDING_AUTO_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [imStatus?.configured, loadPendingRequests]);

  const handleRevoke = async (chatId: string, threadId: number) => {
    if (!groupId) return;
    const key = `${chatId}:${threadId}`;
    setRevoking(key);
    setAuthError("");
    setAuthInfo("");
    try {
      const resp = await api.revokeIMChat(groupId, chatId, threadId);
      if (!resp.ok) {
        setAuthError(resp.error?.message || t("imBridge.revokeError", "Failed to revoke chat authorization."));
        return;
      }
      await loadIMAuthState();
    } catch {
      setAuthError(t("imBridge.revokeError", "Failed to revoke chat authorization."));
    } finally {
      setRevoking(null);
    }
  };

  const handleApprovePending = async (request: api.IMPendingRequest) => {
    if (!groupId) return;
    setPendingActionKey(`approve:${request.key}`);
    setPendingError("");
    setAuthError("");
    setAuthInfo("");
    try {
      const resp = await api.bindIMChat(groupId, request.key);
      if (resp.ok) {
        setAuthInfo(t("imBridge.pendingApproveSuccess", "Request approved and chat bound."));
      } else {
        const code = resp.error?.code;
        setPendingError(
          code === "invalid_key"
            ? t("imBridge.bindError", "Key does not exist or has expired")
            : (resp.error?.message || t("imBridge.pendingApproveError", "Failed to approve request.")),
        );
      }
      await loadIMAuthState();
    } catch {
      setPendingError(t("imBridge.pendingApproveError", "Failed to approve request."));
      await loadPendingRequests();
    } finally {
      setPendingActionKey(null);
    }
  };

  const handleRejectPending = async (request: api.IMPendingRequest) => {
    if (!groupId) return;
    setPendingActionKey(`reject:${request.key}`);
    setPendingError("");
    setAuthError("");
    setAuthInfo("");
    try {
      const resp = await api.rejectIMPending(groupId, request.key);
      if (resp.ok) {
        setAuthInfo(t("imBridge.pendingRejectSuccess", "Pending request rejected."));
      } else {
        setPendingError(resp.error?.message || t("imBridge.pendingRejectError", "Failed to reject request."));
      }
      await loadPendingRequests();
    } catch {
      setPendingError(t("imBridge.pendingRejectError", "Failed to reject request."));
    } finally {
      setPendingActionKey(null);
    }
  };

  const handleToggleVerbose = async (chatId: string, threadId: number, verbose: boolean) => {
    if (!groupId) return;
    try {
      const resp = await api.setIMVerbose(groupId, chatId, verbose, threadId);
      if (resp.ok) {
        setAuthChats((prev) =>
          prev.map((c) =>
            c.chat_id === chatId && c.thread_id === threadId ? { ...c, verbose } : c,
          ),
        );
      }
    } catch {
      // silent fail — user can retry
    }
  };

  const maskKey = (value: string) => {
    const key = String(value || "");
    if (key.length <= 8) return key;
    return `${key.slice(0, 4)}...${key.slice(-3)}`;
  };

  return (
    <div className="space-y-5">
      <section className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div>
            <h3 className={sectionTitleClass}>{t("imBridge.title")}</h3>
            <p className="mt-1 max-w-3xl text-xs text-[var(--color-text-muted)]">{t("imBridge.description")}</p>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
      {imStatus && (
        <div className={settingsWorkspacePanelClass(_isDark)}>
          <div className="flex items-center gap-2">
            <span className={`w-2 h-2 rounded-full ${imStatus.running ? "bg-emerald-500" : "bg-gray-400"}`} />
            <span className="text-sm text-[var(--color-text-primary)]">
              {imStatus.running ? t("imBridge.running") : t("imBridge.stopped")}
            </span>
            {imStatus.running && imStatus.pid && (
              <span className="text-xs text-[var(--color-text-muted)]">
                (PID: {imStatus.pid})
              </span>
            )}
          </div>
          {imStatus.configured && (
            <div className="text-xs mt-1 text-[var(--color-text-tertiary)]">
              {t("imBridge.platform")}: {imStatus.platform} • {t("imBridge.subscribers")}: {imStatus.subscribers}
            </div>
          )}
        </div>
      )}

      {/* Configuration */}
      <div className={settingsWorkspacePanelClass(_isDark)}>
        <div>
          <div className={sectionTitleClass}>{t("imBridge.platform")}</div>
          <div className={sectionHintClass}>{t("imBridge.description")}</div>
        </div>
        <div className="mt-4 space-y-3">
        <div>
          <label className={labelClass()}>{t("imBridge.platform")}</label>
          <SelectCombobox
            items={[
              { value: "telegram", label: "Telegram" },
              { value: "slack", label: "Slack" },
              { value: "discord", label: "Discord" },
              { value: "feishu", label: "Feishu/Lark" },
              { value: "dingtalk", label: "DingTalk" },
              { value: "wecom", label: t("imBridge.wecom") },
              { value: "weixin", label: t("imBridge.weixin") },
            ]}
            value={imPlatform}
            onChange={(value) => onPlatformChange(value as IMPlatform)}
            ariaLabel={t("imBridge.platform")}
            className={inputClass()}
          />
        </div>

        {/* Bot Token (Telegram/Slack/Discord) */}
        {needsBotToken && (
          <div>
            <label className={labelClass()}>{getBotTokenLabel()}</label>
            <input
              type="text"
              value={imBotTokenEnv}
              onChange={(e) => setImBotTokenEnv(e.target.value)}
              placeholder={getBotTokenPlaceholder()}
              className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
            />
            <p className="text-xs mt-1 text-[var(--color-text-muted)]">
              {imPlatform === "slack"
                ? t("imBridge.botTokenHintSlack")
                : t("imBridge.botTokenHint")}
            </p>
          </div>
        )}

        {/* App Token (Slack only) */}
        {imPlatform === "slack" && (
          <div>
            <label className={labelClass()}>{t("imBridge.appToken")}</label>
            <input
              type="text"
              value={imAppTokenEnv}
              onChange={(e) => setImAppTokenEnv(e.target.value)}
              placeholder="SLACK_APP_TOKEN (or xapp-...)"
              className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
            />
            <p className="text-xs mt-1 text-[var(--color-text-muted)]">
              {t("imBridge.appTokenHint")}
            </p>
          </div>
        )}

        {/* Feishu fields */}
        {imPlatform === "feishu" && (
          <>
            <div>
              <label className={labelClass()}>{t("imBridge.apiRegion")}</label>
              <SelectCombobox
                items={[
                  { value: "https://open.feishu.cn", label: t("imBridge.feishuCn") },
                  { value: "https://open.larkoffice.com", label: t("imBridge.larkGlobal") },
                ]}
                value={imFeishuDomain}
                onChange={setImFeishuDomain}
                ariaLabel={t("imBridge.apiRegion")}
                className={inputClass()}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.feishuRegionHint")}
              </p>
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                <Trans i18nKey="imBridge.feishuPackageHint" ns="settings" components={[<code />]} />
              </p>
            </div>
            <div>
              <label className={labelClass()}>{t("imBridge.appId")}</label>
              <input
                type="text"
                value={imFeishuAppId}
                onChange={(e) => setImFeishuAppId(e.target.value)}
                placeholder="FEISHU_APP_ID (or cli_xxx...)"
                className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.appIdHint")}
              </p>
            </div>
            <div>
              <label className={labelClass()}>{t("imBridge.appSecret")}</label>
              <input
                type="password"
                value={imFeishuAppSecret}
                onChange={(e) => setImFeishuAppSecret(e.target.value)}
                placeholder="FEISHU_APP_SECRET (or secret)"
                className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.appSecretHint")}
              </p>
            </div>
          </>
        )}

        {/* DingTalk fields */}
        {imPlatform === "dingtalk" && (
          <>
            <div>
              <label className={labelClass()}>{t("imBridge.appKey")}</label>
              <input
                type="text"
                value={imDingtalkAppKey}
                onChange={(e) => setImDingtalkAppKey(e.target.value)}
                placeholder="DINGTALK_APP_KEY (or key)"
                className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.appKeyHint")}
              </p>
            </div>
            <div>
              <label className={labelClass()}>{t("imBridge.appSecret")}</label>
              <input
                type="password"
                value={imDingtalkAppSecret}
                onChange={(e) => setImDingtalkAppSecret(e.target.value)}
                placeholder="DINGTALK_APP_SECRET (or secret)"
                className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.appSecretHint")}
              </p>
            </div>
            <div>
              <label className={labelClass()}>{t("imBridge.robotCode")}</label>
              <input
                type="text"
                value={imDingtalkRobotCode}
                onChange={(e) => setImDingtalkRobotCode(e.target.value)}
                placeholder="DINGTALK_ROBOT_CODE (or robotCode)"
                className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.robotCodeHint")}
              </p>
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                <Trans i18nKey="imBridge.dingtalkPackageHint" ns="settings" components={[<code />]} />
              </p>
            </div>
          </>
        )}

        {/* WeCom fields */}
        {imPlatform === "wecom" && (
          <>
            <div>
              <label className={labelClass()}>{t("imBridge.wecomBotId")}</label>
              <input
                type="text"
                value={imWecomBotId}
                onChange={(e) => setImWecomBotId(e.target.value)}
                placeholder="WECOM_BOT_ID"
                className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.wecomBotIdHint")}
              </p>
            </div>
            <div>
              <label className={labelClass()}>{t("imBridge.wecomSecret")}</label>
              <input
                type="password"
                value={imWecomSecret}
                onChange={(e) => setImWecomSecret(e.target.value)}
                placeholder="WECOM_SECRET (or secret)"
                className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
              />
              <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                {t("imBridge.wecomSecretHint")}
              </p>
            </div>
          </>
        )}

        {imPlatform === "weixin" && (
          <>
            <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-medium text-[var(--color-text-primary)]">
                    {t("imBridge.weixinLoginTitle")}
                  </div>
                  <div className="mt-2 inline-flex rounded-full border border-emerald-500/15 bg-emerald-500/10 px-2.5 py-1 text-xs font-medium text-[var(--color-text-secondary)]">
                    {t("imBridge.weixinLoginStatus")}: {getWeixinStatusLabel()}
                  </div>
                  {weixinLoginStatus?.account_id && (
                    <div className="text-xs mt-2 text-[var(--color-text-muted)]">
                      {t("imBridge.weixinCurrentAccount")}: {weixinLoginStatus.account_id}
                    </div>
                  )}
                </div>
                <div className="flex flex-wrap gap-2">
                  <button
                    onClick={onStartWeixinLogin}
                    disabled={imBusy || !!weixinLoginStatus?.running}
                    className={primaryButtonClass(imBusy)}
                  >
                    {weixinLoggedIn ? t("imBridge.weixinRefreshQr") : t("imBridge.weixinStartLogin")}
                  </button>
                  <button
                    onClick={onLogoutWeixin}
                    disabled={imBusy || !weixinLoggedIn}
                    className={dangerButtonClass()}
                  >
                    {t("imBridge.weixinLogout")}
                  </button>
                </div>
              </div>
              {weixinErrorText && (
                <div className="mt-3 rounded-lg border border-red-500/20 bg-red-500/5 p-3 text-xs text-red-600 dark:text-red-400">
                  <div className="font-medium">{t("imBridge.weixinErrorDetails")}</div>
                  <div className="mt-1 break-words">{weixinErrorText}</div>
                  {weixinLoginStatus?.updated_at && (
                    <div className="mt-2 text-[11px] text-red-500/80 dark:text-red-300/80">
                      {t("imBridge.weixinLastUpdated")}: {weixinLoginStatus.updated_at}
                    </div>
                  )}
                </div>
              )}
              {weixinLoginStatus?.qrcode_url && (
                <div className="mt-3 flex flex-col items-start gap-2">
                  <WeixinQrCode
                    value={weixinLoginStatus.qrcode_url}
                    loadingLabel={t("imBridge.weixinQrRendering")}
                    errorLabel={t("imBridge.weixinQrRenderFailed")}
                  />
                  <div className="flex flex-wrap items-center gap-2 text-xs">
                    <button
                      type="button"
                      onClick={onCopyWeixinQrLink}
                      className={compactSecondaryButtonClass}
                    >
                      {t("imBridge.weixinCopyQrLink")}
                    </button>
                    {weixinQrCopyState === "done" && (
                      <span className="text-emerald-600">{t("imBridge.weixinCopySuccess")}</span>
                    )}
                    {weixinQrCopyState === "failed" && (
                      <span className="text-red-500">{t("imBridge.weixinCopyFailed")}</span>
                    )}
                  </div>
                  <details className={`w-full ${settingsWorkspaceSoftPanelClass(_isDark)} px-3 py-2`}>
                    <summary className="cursor-pointer text-xs text-[var(--color-text-primary)]">
                      {t("imBridge.weixinQrLinkLabel")}
                    </summary>
                    <p className="mt-2 break-all text-xs text-[var(--color-text-muted)]">
                      {weixinLoginStatus.qrcode_url}
                    </p>
                  </details>
                </div>
              )}
              {!weixinLoginStatus?.qrcode_url && weixinLoginStatus?.qr_ascii && (
                <pre className="mt-3 overflow-auto rounded-lg bg-black/5 p-3 text-[10px] leading-none text-[var(--color-text-secondary)]">
                  {weixinLoginStatus.qr_ascii}
                </pre>
              )}
              <p className="text-xs mt-2 text-[var(--color-text-muted)]">
                {getWeixinHint()}
              </p>
              {weixinLoggedIn && (
                <div className={`mt-3 ${settingsWorkspaceSoftPanelClass(_isDark)} text-xs text-[var(--color-text-muted)]`}>
                  <div className="font-medium text-[var(--color-text-primary)]">
                    {t("imBridge.weixinSubscribeTitle")}
                  </div>
                  <p className="mt-1 leading-relaxed">
                    <Trans
                      i18nKey={getWeixinSubscribeBodyKey()}
                      ns="settings"
                      values={{ count: weixinAuthorizedChatCount }}
                      components={[
                        <code className="rounded bg-black/5 px-1 py-0.5 font-mono text-[11px] text-[var(--color-text-secondary)]" />,
                      ]}
                    />
                  </p>
                </div>
              )}
            </div>
            <details className={settingsWorkspaceSoftPanelClass(_isDark)} open={weixinHasCustomAdvanced}>
              <summary className="cursor-pointer text-sm font-medium text-[var(--color-text-primary)]">
                {t("imBridge.weixinAdvanced")}
              </summary>
              <div className="mt-3 space-y-3">
                <div>
                  <label className={labelClass()}>{t("imBridge.weixinAccountId")}</label>
                  <input
                    type="text"
                    value={imWeixinAccountId}
                    onChange={(e) => setImWeixinAccountId(e.target.value)}
                    placeholder={t("imBridge.weixinAccountIdPlaceholder")}
                    className={`${inputClass()} placeholder-[var(--color-text-muted)]`}
                  />
                  <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                    {t("imBridge.weixinAccountIdHint")}
                  </p>
                </div>
                <p className="text-xs mt-1 text-[var(--color-text-muted)]">
                  {t("imBridge.weixinPackageHint")}
                </p>
              </div>
            </details>
          </>
        )}
        </div>
      </div>

      {/* Actions */}
      <div className={settingsWorkspacePanelClass(_isDark)}>
        <div className={sectionTitleClass}>{t("common:actions", { defaultValue: "Actions" })}</div>
        <div className="mt-4 flex flex-wrap gap-2">
        <button
          onClick={onSaveConfig}
          disabled={imBusy || !canSaveIM()}
          className={primaryButtonClass(imBusy)}
        >
          {imBusy ? t("common:saving") : t("imBridge.saveConfig")}
        </button>

        {imStatus?.configured && (
          <>
            {imStatus.running ? (
              <button
                onClick={onStopBridge}
                disabled={imBusy}
                className={dangerButtonClass()}
              >
                {t("imBridge.stopBridge")}
              </button>
            ) : (
              <button
                onClick={onStartBridge}
                disabled={imBusy}
                className={primaryButtonClass(imBusy)}
              >
                {t("imBridge.startBridge")}
              </button>
            )}

            <button
              onClick={onRemoveConfig}
              disabled={imBusy}
              className={secondaryButtonClass()}
            >
              {t("imBridge.removeConfig")}
            </button>
          </>
        )}
      </div>
      </div>

      {/* Pending Requests */}
      {imStatus?.configured && (
        <div className={settingsWorkspacePanelClass(_isDark)}>
          <div className="flex items-center justify-between">
            <h3 className={sectionTitleClass}>
              {t("imBridge.pendingRequests", "Pending Requests")}
            </h3>
            <div className="flex items-center gap-1">
              <button
                onClick={loadIMAuthState}
                disabled={pendingLoading || authLoading}
                className={compactSecondaryButtonClass}
              >
                {pendingLoading || authLoading ? "..." : "↻"}
              </button>
            </div>
          </div>

          {pendingError && (
            <p className="text-xs text-red-500">{pendingError}</p>
          )}

          {!pendingLoading && pendingRequests.length === 0 && !pendingError && (
            <p className="text-xs text-[var(--color-text-muted)]">
              {t("imBridge.noPendingRequests", "No pending requests.")}
            </p>
          )}

          {pendingRequests.length > 0 && (
            <div className={`${settingsWorkspaceSoftPanelClass(_isDark)} mt-3 space-y-0 divide-y divide-[var(--glass-border-subtle)]`}>
              {pendingRequests.map((request) => {
                const approveKey = `approve:${request.key}`;
                const rejectKey = `reject:${request.key}`;
                const actionBusy = pendingActionKey === approveKey || pendingActionKey === rejectKey;
                return (
                  <div key={request.key} className="flex items-center justify-between py-2 first:pt-0 last:pb-0 gap-2">
                    <div className="min-w-0 flex-1">
                      <div className="text-sm truncate text-[var(--color-text-secondary)]">
                        {request.chat_id}
                        {request.thread_id ? ` (thread: ${request.thread_id})` : ""}
                      </div>
                      <div className="text-xs text-[var(--color-text-muted)]">
                        {request.platform}
                        {` • `}
                        {t("imBridge.pendingKey", "key")}: {maskKey(request.key)}
                        {` • `}
                        {t("imBridge.expiresIn", { seconds: Math.max(0, Math.floor(request.expires_in_seconds || 0)), defaultValue: "expires in {{seconds}}s" })}
                      </div>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      <button
                        onClick={() => handleApprovePending(request)}
                        disabled={actionBusy}
                        className="inline-flex items-center justify-center rounded-lg border border-emerald-500/25 bg-emerald-500/12 px-3 py-1.5 text-xs font-medium text-emerald-700 transition-colors hover:bg-emerald-500/18 dark:text-emerald-300 disabled:opacity-50"
                      >
                        {pendingActionKey === approveKey ? "..." : t("imBridge.approve", "Approve")}
                      </button>
                      <button
                        onClick={() => handleRejectPending(request)}
                        disabled={actionBusy}
                        className={compactDangerButtonClass}
                      >
                        {pendingActionKey === rejectKey ? "..." : t("imBridge.rejectPending", "Reject")}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Authorized Chats */}
      {imStatus?.configured && (
        <div className={settingsWorkspacePanelClass(_isDark)}>
          <div className="flex items-center justify-between">
            <h3 className={sectionTitleClass}>
              {t("imBridge.authorizedChats", "Authorized Chats")}
            </h3>
            <div className="flex items-center gap-1">
              <button
                onClick={async () => {
                  setAuthError("");
                  setPendingError("");
                  try {
                    const ok = await copyTextToClipboard("/subscribe");
                    if (ok) {
                      setAuthInfo(t("imBridge.requestCopied", "Copied /subscribe. Send it in your IM chat to request a key, then approve it from Pending Requests (or bind by key)."));
                    } else {
                      setAuthInfo(t("imBridge.requestHint", "Step 1: In your IM chat, send /subscribe to request a temporary key. Step 2: the request will appear below in Pending Requests; click Approve (or paste the key in Bind). If foreman is online, you can forward the key and ask foreman to bind it for you."));
                    }
                  } catch {
                    setAuthInfo(t("imBridge.requestHint", "Step 1: In your IM chat, send /subscribe to request a temporary key. Step 2: the request will appear below in Pending Requests; click Approve (or paste the key in Bind). If foreman is online, you can forward the key and ask foreman to bind it for you."));
                  }
                }}
                className={compactSecondaryButtonClass}
              >
                {t("imBridge.requestKey", "Request Key")}
              </button>
              <button
                onClick={() => { setShowBindInput(v => !v); setBindKey(""); setAuthError(""); setAuthInfo(""); }}
                className={compactSecondaryButtonClass}
              >
                + {t("imBridge.bind", "Bind")}
              </button>
              <button
                onClick={loadIMAuthState}
                disabled={authLoading}
                className={compactSecondaryButtonClass}
              >
                {authLoading ? "..." : "↻"}
              </button>
            </div>
          </div>

          {showBindInput && (
            <div className={`${settingsWorkspaceSoftPanelClass(_isDark)} mt-3 flex items-center gap-2`}>
              <span className="text-xs shrink-0 text-[var(--color-text-tertiary)]">
                {t("imBridge.bindKey", "Key")}:
              </span>
              <input
                type="text"
                value={bindKey}
                onChange={(e) => setBindKey(e.target.value)}
                placeholder={t("imBridge.bindPlaceholder", "Paste bind key")}
                className={`${inputClass()} flex-1 text-xs`}
                disabled={binding}
              />
              <button
                onClick={async () => {
                  if (!groupId || !bindKey.trim()) return;
                  setBinding(true);
                  setAuthError("");
                  setAuthInfo("");
                  try {
                    const resp = await api.bindIMChat(groupId, bindKey.trim());
                    if (resp.ok) {
                      setShowBindInput(false);
                      setBindKey("");
                      setAuthInfo(t("imBridge.bindSuccess", "Chat bound successfully."));
                      await loadIMAuthState();
                    } else {
                      const code = resp.error?.code;
                      setAuthError(
                        code === "invalid_key"
                          ? t("imBridge.bindError", "Key does not exist or has expired")
                          : (resp.error?.message || t("imBridge.bindFailed")),
                      );
                    }
                  } catch {
                    setAuthError(t("imBridge.bindError", "Key does not exist or has expired"));
                  } finally {
                    setBinding(false);
                  }
                }}
                disabled={binding || !bindKey.trim()}
                className={`${primaryButtonClass(binding)} min-h-[36px] px-3 py-1 text-xs`}
              >
                {binding ? "..." : t("imBridge.bind", "Bind")}
              </button>
              <button
                onClick={() => { setShowBindInput(false); setBindKey(""); setAuthError(""); setAuthInfo(""); }}
                className={compactSecondaryButtonClass}
              >
                ✕
              </button>
            </div>
          )}

          {authError && (
            <p className="text-xs text-red-500">{authError}</p>
          )}
          {!authError && authInfo && (
            <p className="text-xs text-emerald-600 dark:text-emerald-400">{authInfo}</p>
          )}

          {!authLoading && authChats.length === 0 && !authError && (
            <p className="text-xs text-[var(--color-text-muted)]">
              {t("imBridge.noAuthorizedChats", "No authorized chats yet.")}
            </p>
          )}

          {authChats.length > 0 && (
            <div className={`${settingsWorkspaceSoftPanelClass(_isDark)} mt-3 space-y-0 divide-y divide-[var(--glass-border-subtle)]`}>
              {authChats.map((chat) => {
                const key = `${chat.chat_id}:${chat.thread_id}`;
                const isRevoking = revoking === key;
                return (
                  <div key={key} className="flex items-center justify-between py-2 first:pt-0 last:pb-0">
                    <div className="min-w-0 flex-1">
                      <div className="text-sm truncate text-[var(--color-text-secondary)]">
                        {chat.chat_id}
                        {chat.thread_id ? ` (thread: ${chat.thread_id})` : ""}
                      </div>
                      <div className="text-xs text-[var(--color-text-muted)]">
                        {chat.platform}
                        {chat.authorized_at && ` • ${new Date(chat.authorized_at * 1000).toLocaleDateString()}`}
                      </div>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      <button
                        onClick={() => handleToggleVerbose(chat.chat_id, chat.thread_id, !chat.verbose)}
                        title={chat.verbose
                          ? t("imBridge.verboseOnHint", "Receiving all messages. Click to receive user-only messages.")
                          : t("imBridge.verboseOffHint", "Receiving user-only messages. Click to receive all messages.")}
                        className={`px-2.5 py-1 text-xs rounded-lg transition-colors font-medium ${
                          chat.verbose
                            ? "border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg-hover)]"
                            : "bg-[var(--glass-tab-bg)] hover:bg-[var(--glass-tab-bg-hover)] text-[var(--color-text-tertiary)]"
                        }`}
                      >
                        {chat.verbose
                          ? t("imBridge.verboseAll", "All")
                          : t("imBridge.verboseUserOnly", "User only")}
                      </button>
                      <button
                        onClick={() => handleRevoke(chat.chat_id, chat.thread_id)}
                        disabled={isRevoking}
                        className={compactDangerButtonClass}
                      >
                        {isRevoking ? "..." : t("imBridge.revoke", "Revoke")}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Help */}
      <div className={settingsWorkspacePanelClass(_isDark)}>
        <div className={sectionTitleClass}>{t("common:help", { defaultValue: "Help" })}</div>
        <div className="mt-3 text-xs space-y-1 text-[var(--color-text-muted)]">
        <p>{t("imBridge.setupGuide")}</p>
        <ol className="list-decimal list-inside space-y-0.5 ml-2">
          <li>{t("imBridge.setupStep1")}</li>
          <li>{t("imBridge.setupStep2")}</li>
          <li>{t("imBridge.setupStep3")}</li>
          <li>{t("imBridge.setupStep4")}</li>
        </ol>
      </div>
      </div>
        </div>
      </section>
    </div>
  );
}
