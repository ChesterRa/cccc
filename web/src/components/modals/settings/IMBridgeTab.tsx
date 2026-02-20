// IMBridgeTab configures IM bridge settings.
import { useState, useEffect, useCallback } from "react";
import { useTranslation, Trans } from "react-i18next";
import { IMStatus, IMPlatform } from "../../../types";
import * as api from "../../../services/api";
import { inputClass, labelClass, primaryButtonClass, cardClass } from "./types";

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
  const [revoking, setRevoking] = useState<string | null>(null);

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

  useEffect(() => {
    if (imStatus?.configured) {
      loadAuthorizedChats();
    }
  }, [imStatus?.configured, loadAuthorizedChats]);

  const handleRevoke = async (chatId: string, threadId: number) => {
    if (!groupId) return;
    const key = `${chatId}:${threadId}`;
    setRevoking(key);
    try {
      await api.revokeIMChat(groupId, chatId, threadId);
      await loadAuthorizedChats();
    } catch {
      // ignore — list refresh will show current state
    } finally {
      setRevoking(null);
    }
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

      {/* Authorized Chats */}
      {imStatus?.configured && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>
              {t("imBridge.authorizedChats", "Authorized Chats")}
            </h3>
            <button
              onClick={loadAuthorizedChats}
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

          {authError && (
            <p className="text-xs text-red-500">{authError}</p>
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
                        {chat.authorized_at && ` • ${new Date(chat.authorized_at).toLocaleDateString()}`}
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
