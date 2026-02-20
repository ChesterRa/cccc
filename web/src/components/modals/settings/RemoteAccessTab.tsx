// RemoteAccessTab manages remote access configuration + runtime controls.
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RemoteAccessState } from "../../../types";
import * as api from "../../../services/api";
import { cardClass, preClass } from "./types";

interface RemoteAccessTabProps {
  isDark: boolean;
  isActive?: boolean;
}

export function RemoteAccessTab({ isDark, isActive = true }: RemoteAccessTabProps) {
  const { t } = useTranslation("settings");
  const [state, setState] = useState<RemoteAccessState | null>(null);
  const [provider, setProvider] = useState<"off" | "manual" | "tailscale">("off");
  const [mode, setMode] = useState("tailnet_only");
  const [busy, setBusy] = useState(false);
  const [saveBusy, setSaveBusy] = useState(false);
  const [startBusy, setStartBusy] = useState(false);
  const [stopBusy, setStopBusy] = useState(false);
  const [unsupported, setUnsupported] = useState(false);
  const [err, setErr] = useState("");
  const [hint, setHint] = useState("");

  const loadState = async () => {
    setBusy(true);
    setErr("");
    try {
      const resp = await api.fetchRemoteAccessState();
      if (!resp.ok) {
        const code = String(resp.error?.code || "").trim();
        if (code === "unknown_op") {
          setUnsupported(true);
          setErr(t("remoteAccess.unsupported"));
          return;
        }
        setUnsupported(false);
        setErr(resp.error?.message || t("remoteAccess.loadFailed"));
        return;
      }
      const ra = resp.result?.remote_access;
      if (!ra) {
        setUnsupported(false);
        setErr(t("remoteAccess.loadFailed"));
        return;
      }
      setUnsupported(false);
      setState(ra);
      setProvider((ra.provider === "manual" || ra.provider === "tailscale" || ra.provider === "off" ? ra.provider : "off"));
      setMode(String(ra.mode || "tailnet_only"));
    } catch {
      setUnsupported(false);
      setErr(t("remoteAccess.loadFailed"));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!isActive) return;
    void loadState();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Load when tab becomes active.
  }, [isActive]);

  const setHintWithTimeout = (text: string) => {
    setHint(text);
    window.setTimeout(() => setHint(""), 1800);
  };

  const handleSave = async () => {
    if (unsupported) {
      setErr(t("remoteAccess.unsupported"));
      return;
    }
    setSaveBusy(true);
    setErr("");
    try {
      const resp = await api.updateRemoteAccessConfig({
        provider,
        mode,
        enforceWebToken: true,
      });
      if (!resp.ok) {
        setErr(resp.error?.message || t("remoteAccess.saveFailed"));
        return;
      }
      const ra = resp.result?.remote_access;
      if (ra) setState(ra);
      setHintWithTimeout(t("common:saved"));
    } catch {
      setErr(t("remoteAccess.saveFailed"));
    } finally {
      setSaveBusy(false);
    }
  };

  const handleStart = async () => {
    if (unsupported) {
      setErr(t("remoteAccess.unsupported"));
      return;
    }
    setStartBusy(true);
    setErr("");
    try {
      const resp = await api.startRemoteAccess();
      if (!resp.ok) {
        setErr(resp.error?.message || t("remoteAccess.startFailed"));
        return;
      }
      const ra = resp.result?.remote_access;
      if (ra) setState(ra);
      setHintWithTimeout(t("remoteAccess.started"));
    } catch {
      setErr(t("remoteAccess.startFailed"));
    } finally {
      setStartBusy(false);
    }
  };

  const handleStop = async () => {
    if (unsupported) {
      setErr(t("remoteAccess.unsupported"));
      return;
    }
    setStopBusy(true);
    setErr("");
    try {
      const resp = await api.stopRemoteAccess();
      if (!resp.ok) {
        setErr(resp.error?.message || t("remoteAccess.stopFailed"));
        return;
      }
      const ra = resp.result?.remote_access;
      if (ra) setState(ra);
      setHintWithTimeout(t("remoteAccess.stopped"));
    } catch {
      setErr(t("remoteAccess.stopFailed"));
    } finally {
      setStopBusy(false);
    }
  };

  const copyEndpoint = async () => {
    const endpoint = String(state?.endpoint || "").trim();
    if (!endpoint) return;
    try {
      await navigator.clipboard.writeText(endpoint);
      setHintWithTimeout(t("remoteAccess.copied"));
    } catch {
      setHintWithTimeout(t("common:copyFailed"));
    }
  };

  const status = String(state?.status || "stopped");
  const statusLabel = (() => {
    if (unsupported) return t("remoteAccess.statusUnsupported");
    if (status === "running") return t("remoteAccess.statusRunning");
    if (status === "not_installed") return t("remoteAccess.statusNotInstalled");
    if (status === "not_authenticated") return t("remoteAccess.statusNotAuthenticated");
    if (status === "misconfigured") return t("remoteAccess.statusMisconfigured");
    if (status === "error") return t("remoteAccess.statusError");
    return t("remoteAccess.statusStopped");
  })();
  const diagnostics = unsupported ? null : (state?.diagnostics || null);
  const nextSteps = unsupported ? [] : (Array.isArray(state?.next_steps) ? state?.next_steps.filter((item) => typeof item === "string" && item.trim()) : []);
  const statusClass =
    status === "running"
      ? (isDark ? "text-emerald-300 bg-emerald-900/30 border-emerald-700/60" : "text-emerald-700 bg-emerald-50 border-emerald-200")
      : status === "not_installed" || status === "not_authenticated" || status === "misconfigured" || status === "error"
        ? (isDark ? "text-amber-300 bg-amber-900/30 border-amber-700/60" : "text-amber-700 bg-amber-50 border-amber-200")
        : (isDark ? "text-slate-300 bg-slate-800 border-slate-700" : "text-gray-700 bg-gray-50 border-gray-200");

  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>{t("remoteAccess.title")}</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("remoteAccess.description")}</p>
        <div
          className={`mt-2 rounded-lg border px-3 py-2 text-[11px] ${
            isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
          }`}
        >
          <div className="font-medium">{t("remoteAccess.securityNote")}</div>
          <div className="mt-1">{t("remoteAccess.securityWarning")}</div>
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className="flex items-center justify-between gap-2">
          <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("remoteAccess.controlTitle")}</div>
          <button
            onClick={() => void loadState()}
            disabled={busy || saveBusy || startBusy || stopBusy}
            className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {busy ? t("common:loading") : t("remoteAccess.refresh")}
          </button>
        </div>

        <div className={`mt-3 inline-flex items-center rounded-lg border px-2.5 py-1 text-xs ${statusClass}`}>
          {t("remoteAccess.statusLabel")}: {statusLabel}
        </div>

        {unsupported ? (
          <div className={`mt-2 rounded-lg border px-3 py-2 text-[11px] ${
            isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
          }`}>
            {t("remoteAccess.unsupported")}
          </div>
        ) : null}

        <div className="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2">
          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("remoteAccess.providerLabel")}</label>
            <select
              value={provider}
              onChange={(e) => setProvider((e.target.value as "off" | "manual" | "tailscale") || "off")}
              disabled={unsupported}
              className={`w-full px-3 py-2 text-sm rounded-lg min-h-[44px] ${
                isDark ? "bg-slate-900 border border-slate-700 text-slate-100" : "bg-white border border-gray-300 text-gray-900"
              } disabled:opacity-50`}
            >
              <option value="off">{t("remoteAccess.providerOff")}</option>
              <option value="manual">{t("remoteAccess.providerManual")}</option>
              <option value="tailscale">{t("remoteAccess.providerTailscale")}</option>
            </select>
          </div>

          <div>
            <label className={`block text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("remoteAccess.modeLabel")}</label>
            <select
              value={mode}
              onChange={(e) => setMode(e.target.value || "tailnet_only")}
              disabled={unsupported}
              className={`w-full px-3 py-2 text-sm rounded-lg min-h-[44px] ${
                isDark ? "bg-slate-900 border border-slate-700 text-slate-100" : "bg-white border border-gray-300 text-gray-900"
              } disabled:opacity-50`}
            >
              <option value="tailnet_only">tailnet_only</option>
            </select>
          </div>
        </div>

        <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("remoteAccess.tokenEnforcedHint")}</div>

        {!diagnostics?.web_token_present ? (
          <div className={`mt-2 rounded-lg border px-3 py-2 text-[11px] ${
            isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-200" : "border-rose-200 bg-rose-50 text-rose-700"
          }`}>
            {t("remoteAccess.missingToken")}
          </div>
        ) : null}

        {diagnostics?.web_bind_loopback ? (
          <div className={`mt-2 rounded-lg border px-3 py-2 text-[11px] ${
            isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
          }`}>
            {t("remoteAccess.loopbackBinding", {
              host: diagnostics.web_host || "127.0.0.1",
            })}
          </div>
        ) : null}

        {state?.endpoint ? (
          <div className="mt-3">
            <div className={`text-[11px] mb-1 ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("remoteAccess.endpointLabel")}</div>
            <div className="flex items-center gap-2">
              <code className={`flex-1 rounded px-2 py-1 text-[11px] break-all ${isDark ? "bg-slate-900 text-slate-200" : "bg-gray-100 text-gray-800"}`}>
                {state.endpoint}
              </code>
              <button
                onClick={() => void copyEndpoint()}
                className={`px-3 py-2 rounded-lg text-xs min-h-[40px] transition-colors ${
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
                }`}
              >
                {t("remoteAccess.copy")}
              </button>
            </div>
          </div>
        ) : null}

        {nextSteps.length ? (
          <div className={`mt-3 rounded-lg border px-3 py-2 text-xs ${
            isDark ? "border-slate-700 bg-slate-900/50 text-slate-300" : "border-gray-200 bg-white text-gray-700"
          }`}>
            <div className={`font-medium ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("remoteAccess.nextStepsTitle")}</div>
            <div className="mt-1 space-y-1">
              {nextSteps.map((step, idx) => (
                <div key={`${idx}-${step}`}>{idx + 1}. {step}</div>
              ))}
            </div>
          </div>
        ) : null}

        {err ? <div className={`mt-3 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{err}</div> : null}
        {hint ? <div className={`mt-3 text-xs ${isDark ? "text-emerald-300" : "text-emerald-600"}`}>{hint}</div> : null}

        <div className="mt-3 flex flex-wrap gap-2">
          <button
            onClick={() => void handleSave()}
            disabled={unsupported || saveBusy || startBusy || stopBusy}
            className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
              isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-800 border border-gray-200"
            } disabled:opacity-50`}
          >
            {saveBusy ? t("common:saving") : t("remoteAccess.save")}
          </button>
          <button
            onClick={() => void handleStart()}
            disabled={unsupported || startBusy || saveBusy || stopBusy}
            className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
              isDark ? "bg-emerald-900/40 hover:bg-emerald-800/40 text-emerald-300 border border-emerald-700/60" : "bg-emerald-50 hover:bg-emerald-100 text-emerald-700 border border-emerald-200"
            } disabled:opacity-50`}
          >
            {startBusy ? t("common:loading") : t("remoteAccess.start")}
          </button>
          <button
            onClick={() => void handleStop()}
            disabled={unsupported || stopBusy || saveBusy || startBusy}
            className={`px-3 py-2 rounded-lg text-sm min-h-[44px] font-medium transition-colors ${
              isDark ? "bg-rose-900/40 hover:bg-rose-800/40 text-rose-300 border border-rose-700/60" : "bg-rose-50 hover:bg-rose-100 text-rose-700 border border-rose-200"
            } disabled:opacity-50`}
          >
            {stopBusy ? t("common:loading") : t("remoteAccess.stop")}
          </button>
        </div>
      </div>

      <div className={cardClass(isDark)}>
        <div className={`text-sm font-semibold ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t("remoteAccess.tailscaleQuickTitle")}</div>
        <pre className={preClass(isDark)}>
          <code>{`tailscale up
TAILSCALE_IP=$(tailscale ip -4)
CCCC_WEB_HOST=$TAILSCALE_IP CCCC_WEB_PORT=8848 cccc`}</code>
        </pre>
      </div>
    </div>
  );
}
