// RemoteAccessTab provides remote access setup guidance.
import { useTranslation } from "react-i18next";
import { cardClass, preClass } from "./types";

interface RemoteAccessTabProps {
  isDark: boolean;
}

export function RemoteAccessTab({ isDark }: RemoteAccessTabProps) {
  const { t } = useTranslation("settings");

  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("remoteAccess.title")}</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          {t("remoteAccess.description")}
        </p>
        <div className={`mt-2 rounded-lg border px-3 py-2 text-[11px] ${
          isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
        }`}>
          <div className="font-medium">{t("remoteAccess.securityNote")}</div>
          <div className="mt-1">
            {t("remoteAccess.securityWarning")}
          </div>
        </div>
      </div>

      {/* Cloudflare Tunnel */}
      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("remoteAccess.cloudflareTunnel")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
          {t("remoteAccess.cloudflareHint")}
        </div>

        <div className={`mt-3 text-xs font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("remoteAccess.quickTemporary")}</div>
        <pre className={preClass(isDark)}>
          <code>{`# Install cloudflared first, then:
cloudflared tunnel --url http://127.0.0.1:8848
# It will print a https://....trycloudflare.com URL`}</code>
        </pre>

        <div className={`mt-3 text-xs font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("remoteAccess.stableDomain")}</div>
        <pre className={preClass(isDark)}>
          <code>{`# 1) Authenticate
cloudflared tunnel login

# 2) Create a named tunnel
cloudflared tunnel create cccc

# 3) Route DNS (replace with your hostname)
cloudflared tunnel route dns cccc cccc.example.com

# 4) Create ~/.cloudflared/config.yml (example):
# tunnel: <TUNNEL-UUID>
# credentials-file: /home/<you>/.cloudflared/<TUNNEL-UUID>.json
# ingress:
#   - hostname: cccc.example.com
#     service: http://127.0.0.1:8848
#   - service: http_status:404

# 5) Run
cloudflared tunnel run cccc`}</code>
        </pre>

        <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-600"}`}>
          {t("remoteAccess.cloudflareTip")}
        </div>
      </div>

      {/* Tailscale */}
      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("remoteAccess.tailscale")}</div>
        <div className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-600"}`}>
          {t("remoteAccess.tailscaleHint")}
        </div>
        <pre className={preClass(isDark)}>
          <code>{`# 1) Install Tailscale on the server + phone, then on the server:
tailscale up

# 2) Get your tailnet IP
TAILSCALE_IP=$(tailscale ip -4)

# 3) Bind Web UI to that IP (so it's only reachable via tailnet)
CCCC_WEB_HOST=$TAILSCALE_IP CCCC_WEB_PORT=8848 cccc

# 4) On phone browser:
# http://<TAILSCALE_IP>:8848/ui/`}</code>
        </pre>
      </div>

      <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        {t("remoteAccess.phoneTip")}
      </div>
    </div>
  );
}
