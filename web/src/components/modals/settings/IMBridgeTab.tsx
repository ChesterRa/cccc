// IMBridgeTab - IM 桥接设置
import { IMStatus } from "../../../types";
import { inputClass, labelClass, primaryButtonClass, cardClass } from "./types";

interface IMBridgeTabProps {
  isDark: boolean;
  groupId?: string; // 保留用于未来扩展
  imStatus: IMStatus | null;
  imPlatform: "telegram" | "slack" | "discord";
  setImPlatform: (v: "telegram" | "slack" | "discord") => void;
  imBotTokenEnv: string;
  setImBotTokenEnv: (v: string) => void;
  imAppTokenEnv: string;
  setImAppTokenEnv: (v: string) => void;
  imBusy: boolean;
  onSaveConfig: () => void;
  onRemoveConfig: () => void;
  onStartBridge: () => void;
  onStopBridge: () => void;
}

export function IMBridgeTab({
  isDark,
  groupId: _groupId,
  imStatus,
  imPlatform,
  setImPlatform,
  imBotTokenEnv,
  setImBotTokenEnv,
  imAppTokenEnv,
  setImAppTokenEnv,
  imBusy,
  onSaveConfig,
  onRemoveConfig,
  onStartBridge,
  onStopBridge,
}: IMBridgeTabProps) {
  const getBotTokenLabel = () => {
    switch (imPlatform) {
      case "telegram": return "Bot Token (token or env var)";
      case "slack": return "Bot Token (xoxb- or env var)";
      case "discord": return "Bot Token (token or env var)";
    }
  };

  const getBotTokenPlaceholder = () => {
    switch (imPlatform) {
      case "telegram": return "TELEGRAM_BOT_TOKEN (or 123456:ABC...)";
      case "slack": return "SLACK_BOT_TOKEN (or xoxb-...)";
      case "discord": return "DISCORD_BOT_TOKEN (or <token>)";
    }
  };

  const canSaveIM = () => {
    if (!imBotTokenEnv) return false;
    if (imPlatform === "slack" && !imAppTokenEnv) return false;
    return true;
  };

  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>IM Bridge</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          Connect this group to Telegram, Slack, or Discord.
        </p>
      </div>

      {/* Status */}
      {imStatus && (
        <div className={cardClass(isDark)}>
          <div className="flex items-center gap-2">
            <span className={`w-2 h-2 rounded-full ${imStatus.running ? "bg-emerald-500" : "bg-gray-400"}`} />
            <span className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}>
              {imStatus.running ? "Running" : "Stopped"}
            </span>
            {imStatus.running && imStatus.pid && (
              <span className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                (PID: {imStatus.pid})
              </span>
            )}
          </div>
          {imStatus.configured && (
            <div className={`text-xs mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
              Platform: {imStatus.platform} • Subscribers: {imStatus.subscribers}
            </div>
          )}
        </div>
      )}

      {/* Configuration */}
      <div className="space-y-3">
        <div>
          <label className={labelClass(isDark)}>Platform</label>
          <select
            value={imPlatform}
            onChange={(e) => setImPlatform(e.target.value as "telegram" | "slack" | "discord")}
            className={inputClass(isDark)}
          >
            <option value="telegram">Telegram</option>
            <option value="slack">Slack</option>
            <option value="discord">Discord</option>
          </select>
        </div>

        {/* Bot Token */}
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
              ? "Paste xoxb-… token or an env var name; required for outbound messages."
              : "Paste the bot token or an env var name; required for bot authentication."}
          </p>
        </div>

        {/* App Token (Slack only) */}
        {imPlatform === "slack" && (
          <div>
            <label className={labelClass(isDark)}>App Token (xapp- or env var)</label>
            <input
              type="text"
              value={imAppTokenEnv}
              onChange={(e) => setImAppTokenEnv(e.target.value)}
              placeholder="SLACK_APP_TOKEN (or xapp-...)"
              className={`${inputClass(isDark)} placeholder:${isDark ? "text-slate-600" : "text-gray-400"}`}
            />
            <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
              Optional; needed for inbound messages (Socket Mode).
            </p>
          </div>
        )}
      </div>

      {/* Actions */}
      <div className="flex flex-wrap gap-2">
        <button
          onClick={onSaveConfig}
          disabled={imBusy || !canSaveIM()}
          className={primaryButtonClass(imBusy)}
        >
          {imBusy ? "Saving..." : "Save Config"}
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
                Stop Bridge
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
                Start Bridge
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
              Remove Config
            </button>
          </>
        )}
      </div>

      {/* Help */}
      <div className={`text-xs space-y-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        <p>To use IM Bridge:</p>
        <ol className="list-decimal list-inside space-y-0.5 ml-2">
          <li>Create a bot on your IM platform</li>
          <li>Set the token(s) as environment variable(s)</li>
          <li>Save the config and start the bridge</li>
          <li>In your IM chat, send /subscribe to receive messages</li>
        </ol>
      </div>
    </div>
  );
}
