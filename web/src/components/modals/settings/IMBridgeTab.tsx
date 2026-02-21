// IMBridgeTab configures IM bridge settings.
import { useState, useEffect, useCallback } from "react";
import { useTranslation, Trans } from "react-i18next";
import { IMStatus, IMPlatform } from "../../../types";
import * as api from "../../../services/api";
import { inputClass, labelClass, primaryButtonClass, cardClass } from "./types";

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
  // Actions
  imBusy: boolean;
  onSaveConfig: () => void;
  onRemoveConfig: () => void;
  onStartBridge: () => void;
  onStopBridge: () => void;
}

export function IMBridgeTab({
  isDark,
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
  imBusy,
  onSaveConfig,
  onRemoveConfig,
  onStartBridge,
  onStopBridge,
}: IMBridgeTabProps) {
  const { t } = useTranslation("settings");
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

  const loadAuthorizedChats = useCallback(async () => {
    if (!groupId) return;
    setAuthLoading(true);
    setAuthError("");
    try {
      const resp = await api.fetchIMAuthorized(groupId);
      if (resp.ok) {
        setAuthChats(resp.result?.authorized ?? []);
      } else {
        setAuthError(resp.error?.message || "Failed to load authorized chats");
      }
    } catch {
      setAuthError("Failed to load authorized chats");
    } finally {
      setAuthLoading(false);
    }
  }, [groupId]);

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
          setPendingError(resp.error?.message || "Failed to load pending requests");
        }
      }
    } catch {
      if (!silent) {
        setPendingError("Failed to load pending requests");
      }
    } finally {
      if (!silent) {
        setPendingLoading(false);
      }
    }
  }, [groupId]);

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
    try {
      await api.revokeIMChat(groupId, chatId, threadId);
      await loadIMAuthState();
    } catch {
      // ignore — list refresh will show current state
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

  const maskKey = (value: string) => {
    const key = String(value || "");
    if (key.length <= 8) return key;
    return `${key.slice(0, 4)}...${key.slice(-3)}`;
  };

  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("imBridge.title")}</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("imBridge.description")}
        </p>
      </div>

      {/* Status */}
      {imStatus && (
        <div className={cardClass(isDark)}>
          <div className="flex items-center gap-2">
            <span className={`w-2 h-2 rounded-full ${imStatus.running ? "bg-emerald-500" : "bg-gray-400"}`} />
            <span className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}>
              {imStatus.running ? t("imBridge.running") : t("imBridge.stopped")}
            </span>
            {imStatus.running && imStatus.pid && (
              <span className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                (PID: {imStatus.pid})
              </span>
            )}
          </div>
          {imStatus.configured && (
            <div className={`text-xs mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
              {t("imBridge.platform")}: {imStatus.platform} • {t("imBridge.subscribers")}: {imStatus.subscribers}
            </div>
          )}
        </div>
      )}

      {/* Configuration */}
      <div className="space-y-3">
        <div>
          <label className={labelClass(isDark)}>{t("imBridge.platform")}</label>
          <select
            value={imPlatform}
            onChange={(e) => onPlatformChange(e.target.value as IMPlatform)}
            className={inputClass(isDark)}
          >
            <option value="telegram">Telegram</option>
            <option value="slack">Slack</option>
            <option value="discord">Discord</option>
            <option value="feishu">Feishu/Lark</option>
            <option value="dingtalk">DingTalk</option>
          </select>
        </div>

        {/* Bot Token (Telegram/Slack/Discord) */}
        {needsBotToken && (
          <div>
            <label className={labelClass(isDark)}>{getBotTokenLabel()}</label>
            <input
              type="text"
              value={imBotTokenEnv}
              onChange={(e) => setImBotTokenEnv(e.target.value)}
              placeholder={getBotTokenPlaceholder()}
              className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
            />
            <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
              {imPlatform === "slack"
                ? t("imBridge.botTokenHintSlack")
                : t("imBridge.botTokenHint")}
            </p>
          </div>
        )}

        {/* App Token (Slack only) */}
        {imPlatform === "slack" && (
          <div>
            <label className={labelClass(isDark)}>{t("imBridge.appToken")}</label>
            <input
              type="text"
              value={imAppTokenEnv}
              onChange={(e) => setImAppTokenEnv(e.target.value)}
              placeholder="SLACK_APP_TOKEN (or xapp-...)"
              className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
            />
            <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
              {t("imBridge.appTokenHint")}
            </p>
          </div>
        )}

        {/* Feishu fields */}
        {imPlatform === "feishu" && (
          <>
            <div>
              <label className={labelClass(isDark)}>{t("imBridge.apiRegion")}</label>
              <select
                value={imFeishuDomain}
                onChange={(e) => setImFeishuDomain(e.target.value)}
                className={inputClass(isDark)}
              >
                <option value="https://open.feishu.cn">{t("imBridge.feishuCn")}</option>
                <option value="https://open.larkoffice.com">{t("imBridge.larkGlobal")}</option>
              </select>
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                {t("imBridge.feishuRegionHint")}
              </p>
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                <Trans i18nKey="imBridge.feishuPackageHint" ns="settings" components={[<code />]} />
              </p>
            </div>
            <div>
              <label className={labelClass(isDark)}>{t("imBridge.appId")}</label>
              <input
                type="text"
                value={imFeishuAppId}
                onChange={(e) => setImFeishuAppId(e.target.value)}
                placeholder="FEISHU_APP_ID (or cli_xxx...)"
                className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
              />
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                {t("imBridge.appIdHint")}
              </p>
            </div>
            <div>
              <label className={labelClass(isDark)}>{t("imBridge.appSecret")}</label>
              <input
                type="password"
                value={imFeishuAppSecret}
                onChange={(e) => setImFeishuAppSecret(e.target.value)}
                placeholder="FEISHU_APP_SECRET (or secret)"
                className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
              />
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                {t("imBridge.appSecretHint")}
              </p>
            </div>
          </>
        )}

        {/* DingTalk fields */}
        {imPlatform === "dingtalk" && (
          <>
            <div>
              <label className={labelClass(isDark)}>{t("imBridge.appKey")}</label>
              <input
                type="text"
                value={imDingtalkAppKey}
                onChange={(e) => setImDingtalkAppKey(e.target.value)}
                placeholder="DINGTALK_APP_KEY (or key)"
                className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
              />
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                {t("imBridge.appKeyHint")}
              </p>
            </div>
            <div>
              <label className={labelClass(isDark)}>{t("imBridge.appSecret")}</label>
              <input
                type="password"
                value={imDingtalkAppSecret}
                onChange={(e) => setImDingtalkAppSecret(e.target.value)}
                placeholder="DINGTALK_APP_SECRET (or secret)"
                className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
              />
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                {t("imBridge.appSecretHint")}
              </p>
            </div>
            <div>
              <label className={labelClass(isDark)}>{t("imBridge.robotCode")}</label>
              <input
                type="text"
                value={imDingtalkRobotCode}
                onChange={(e) => setImDingtalkRobotCode(e.target.value)}
                placeholder="DINGTALK_ROBOT_CODE (or robotCode)"
                className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
              />
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                {t("imBridge.robotCodeHint")}
              </p>
              <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                <Trans i18nKey="imBridge.dingtalkPackageHint" ns="settings" components={[<code />]} />
              </p>
            </div>
          </>
        )}
      </div>

      {/* Actions */}
      <div className="flex flex-wrap gap-2">
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
                className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors font-medium ${
                  isDark
                    ? "bg-red-900/50 hover:bg-red-800/50 text-red-300"
                    : "bg-red-100 hover:bg-red-200 text-red-700"
                } disabled:opacity-50`}
              >
                {t("imBridge.stopBridge")}
              </button>
            ) : (
              <button
                onClick={onStartBridge}
                disabled={imBusy}
                className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors font-medium ${
                  isDark
                    ? "bg-blue-900/50 hover:bg-blue-800/50 text-blue-300"
                    : "bg-blue-100 hover:bg-blue-200 text-blue-700"
                } disabled:opacity-50`}
              >
                {t("imBridge.startBridge")}
              </button>
            )}

            <button
              onClick={onRemoveConfig}
              disabled={imBusy}
              className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors font-medium ${
                isDark
                  ? "bg-slate-800 hover:bg-slate-700 text-slate-300"
                  : "bg-gray-200 hover:bg-gray-300 text-gray-700"
              } disabled:opacity-50`}
            >
              {t("imBridge.removeConfig")}
            </button>
          </>
        )}
      </div>

      {/* Pending Requests */}
      {imStatus?.configured && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>
              {t("imBridge.pendingRequests", "Pending Requests")}
            </h3>
            <div className="flex items-center gap-1">
              <button
                onClick={loadIMAuthState}
                disabled={pendingLoading || authLoading}
                className={`text-xs px-2 py-1 rounded transition-colors ${
                  isDark
                    ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800"
                    : "text-gray-500 hover:text-gray-700 hover:bg-gray-100"
                } disabled:opacity-50`}
              >
                {pendingLoading || authLoading ? "..." : "↻"}
              </button>
            </div>
          </div>

          {pendingError && (
            <p className="text-xs text-red-500">{pendingError}</p>
          )}

          {!pendingLoading && pendingRequests.length === 0 && !pendingError && (
            <p className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {t("imBridge.noPendingRequests", "No pending requests.")}
            </p>
          )}

          {pendingRequests.length > 0 && (
            <div className={`${cardClass(isDark)} space-y-0 divide-y ${isDark ? "divide-slate-700" : "divide-gray-200"}`}>
              {pendingRequests.map((request) => {
                const approveKey = `approve:${request.key}`;
                const rejectKey = `reject:${request.key}`;
                const actionBusy = pendingActionKey === approveKey || pendingActionKey === rejectKey;
                return (
                  <div key={request.key} className="flex items-center justify-between py-2 first:pt-0 last:pb-0 gap-2">
                    <div className="min-w-0 flex-1">
                      <div className={`text-sm truncate ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                        {request.chat_id}
                        {request.thread_id ? ` (thread: ${request.thread_id})` : ""}
                      </div>
                      <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
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
                        className={`px-3 py-1 text-xs rounded-lg transition-colors font-medium ${
                          isDark
                            ? "bg-emerald-900/40 hover:bg-emerald-800/50 text-emerald-300"
                            : "bg-emerald-50 hover:bg-emerald-100 text-emerald-700"
                        } disabled:opacity-50`}
                      >
                        {pendingActionKey === approveKey ? "..." : t("imBridge.approve", "Approve")}
                      </button>
                      <button
                        onClick={() => handleRejectPending(request)}
                        disabled={actionBusy}
                        className={`px-3 py-1 text-xs rounded-lg transition-colors font-medium ${
                          isDark
                            ? "bg-red-900/40 hover:bg-red-800/50 text-red-400"
                            : "bg-red-50 hover:bg-red-100 text-red-600"
                        } disabled:opacity-50`}
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
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>
              {t("imBridge.authorizedChats", "Authorized Chats")}
            </h3>
            <div className="flex items-center gap-1">
              <button
                onClick={async () => {
                  setAuthError("");
                  setPendingError("");
                  try {
                    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
                      await navigator.clipboard.writeText("/subscribe");
                      setAuthInfo(t("imBridge.requestCopied", "Copied /subscribe. Send it in your IM chat to request a key, then approve it from Pending Requests (or bind by key)."));
                    } else {
                      setAuthInfo(t("imBridge.requestHint", "Step 1: In your IM chat, send /subscribe to request a temporary key. Step 2: the request will appear below in Pending Requests; click Approve (or paste the key in Bind). If foreman is online, you can forward the key and ask foreman to bind it for you."));
                    }
                  } catch {
                    setAuthInfo(t("imBridge.requestHint", "Step 1: In your IM chat, send /subscribe to request a temporary key. Step 2: the request will appear below in Pending Requests; click Approve (or paste the key in Bind). If foreman is online, you can forward the key and ask foreman to bind it for you."));
                  }
                }}
                className={`text-xs px-2 py-1 rounded transition-colors ${
                  isDark
                    ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800"
                    : "text-gray-500 hover:text-gray-700 hover:bg-gray-100"
                }`}
              >
                {t("imBridge.requestKey", "Request Key")}
              </button>
              <button
                onClick={() => { setShowBindInput(v => !v); setBindKey(""); setAuthError(""); setAuthInfo(""); }}
                className={`text-xs px-2 py-1 rounded transition-colors ${
                  isDark
                    ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800"
                    : "text-gray-500 hover:text-gray-700 hover:bg-gray-100"
                }`}
              >
                + {t("imBridge.bind", "Bind")}
              </button>
              <button
                onClick={loadIMAuthState}
                disabled={authLoading}
                className={`text-xs px-2 py-1 rounded transition-colors ${
                  isDark
                    ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800"
                    : "text-gray-500 hover:text-gray-700 hover:bg-gray-100"
                } disabled:opacity-50`}
              >
                {authLoading ? "..." : "↻"}
              </button>
            </div>
          </div>

          {showBindInput && (
            <div className="flex items-center gap-2">
              <span className={`text-xs shrink-0 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
                {t("imBridge.bindKey", "Key")}:
              </span>
              <input
                type="text"
                value={bindKey}
                onChange={(e) => setBindKey(e.target.value)}
                placeholder={t("imBridge.bindPlaceholder", "Paste bind key")}
                className={`${inputClass(isDark)} flex-1 text-xs`}
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
                          : (resp.error?.message || "Bind failed"),
                      );
                    }
                  } catch {
                    setAuthError(t("imBridge.bindError", "Key does not exist or has expired"));
                  } finally {
                    setBinding(false);
                  }
                }}
                disabled={binding || !bindKey.trim()}
                className={`px-3 py-1 text-xs rounded-lg transition-colors font-medium shrink-0 ${
                  isDark
                    ? "bg-blue-900/40 hover:bg-blue-800/50 text-blue-400"
                    : "bg-blue-50 hover:bg-blue-100 text-blue-600"
                } disabled:opacity-50`}
              >
                {binding ? "..." : t("imBridge.bind", "Bind")}
              </button>
              <button
                onClick={() => { setShowBindInput(false); setBindKey(""); setAuthError(""); setAuthInfo(""); }}
                className={`text-xs px-1 py-1 rounded transition-colors ${
                  isDark
                    ? "text-slate-400 hover:text-slate-200"
                    : "text-gray-500 hover:text-gray-700"
                }`}
              >
                ✕
              </button>
            </div>
          )}

          {authError && (
            <p className="text-xs text-red-500">{authError}</p>
          )}
          {!authError && authInfo && (
            <p className={`text-xs ${isDark ? "text-emerald-400" : "text-emerald-600"}`}>{authInfo}</p>
          )}

          {!authLoading && authChats.length === 0 && !authError && (
            <p className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {t("imBridge.noAuthorizedChats", "No authorized chats yet.")}
            </p>
          )}

          {authChats.length > 0 && (
            <div className={`${cardClass(isDark)} space-y-0 divide-y ${isDark ? "divide-slate-700" : "divide-gray-200"}`}>
              {authChats.map((chat) => {
                const key = `${chat.chat_id}:${chat.thread_id}`;
                const isRevoking = revoking === key;
                return (
                  <div key={key} className="flex items-center justify-between py-2 first:pt-0 last:pb-0">
                    <div className="min-w-0 flex-1">
                      <div className={`text-sm truncate ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                        {chat.chat_id}
                        {chat.thread_id ? ` (thread: ${chat.thread_id})` : ""}
                      </div>
                      <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                        {chat.platform}
                        {chat.authorized_at && ` • ${new Date(chat.authorized_at * 1000).toLocaleDateString()}`}
                      </div>
                    </div>
                    <button
                      onClick={() => handleRevoke(chat.chat_id, chat.thread_id)}
                      disabled={isRevoking}
                      className={`ml-3 px-3 py-1 text-xs rounded-lg transition-colors font-medium shrink-0 ${
                        isDark
                          ? "bg-red-900/40 hover:bg-red-800/50 text-red-400"
                          : "bg-red-50 hover:bg-red-100 text-red-600"
                      } disabled:opacity-50`}
                    >
                      {isRevoking ? "..." : t("imBridge.revoke", "Revoke")}
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Help */}
      <div className={`text-xs space-y-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        <p>{t("imBridge.setupGuide")}</p>
        <ol className="list-decimal list-inside space-y-0.5 ml-2">
          <li>{t("imBridge.setupStep1")}</li>
          <li>{t("imBridge.setupStep2")}</li>
          <li>{t("imBridge.setupStep3")}</li>
          <li>{t("imBridge.setupStep4")}</li>
        </ol>
      </div>
    </div>
  );
}
