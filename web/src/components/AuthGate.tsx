import React, { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from 'react-i18next';
import { useTheme } from "../hooks/useTheme";
import * as api from "../services/api";

type AuthStatus = "checking" | "authenticated" | "login";

function needsTokenLogin(resp: api.ApiResponse<unknown>): boolean {
  return !resp.ok && (resp.error?.code === "unauthorized" || resp.error?.code === "permission_denied");
}

export function AuthGate({ children }: { children: React.ReactNode }) {
  const { isDark } = useTheme();
  const [status, setStatus] = useState<AuthStatus>("checking");
  const [token, setToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const calledRef = useRef(false);
  const { t } = useTranslation('layout');

  // 启动时探测受保护接口：若返回 401/403，说明 token 认证已启用且当前尚未登录。
  useEffect(() => {
    if (calledRef.current) return;
    calledRef.current = true;
    api.fetchGroups().then((resp) => {
      if (resp.ok) {
        setStatus("authenticated");
      } else if (needsTokenLogin(resp)) {
        api.clearAuthToken();
        setStatus("login");
      } else {
        // 服务不可达等其他问题先不拦截，交给 App 自己处理。
        setStatus("authenticated");
      }
    });
  }, []);

  // Subscribe to mid-session 401s so the gate re-appears.
  useEffect(() => {
    api.onAuthRequired(() => {
      api.clearAuthToken();
      setStatus("login");
    });
  }, []);

  const handleSubmit = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = token.trim();
    if (!trimmed) return;
    setSubmitting(true);
    setError("");
    api.setAuthToken(trimmed);
    const resp = await api.fetchGroups();
    setSubmitting(false);
    if (resp.ok) {
      setStatus("authenticated");
    } else {
      api.clearAuthToken();
      setError(
        needsTokenLogin(resp)
          ? t('tokenIncorrect')
          : resp.error?.message || t('connectionFailed'),
      );
    }
  }, [token, t]);

  if (status === "checking") {
    return (
      <div className={`fixed inset-0 flex items-center justify-center ${
        isDark ? "bg-slate-950" : "bg-slate-50"
      }`}>
        <div className={`text-sm ${isDark ? "text-slate-400" : "text-slate-500"}`}>
          {t('connecting')}
        </div>
      </div>
    );
  }

  if (status === "authenticated") {
    return <>{children}</>;
  }

  // Login form
  return (
    <div className={`fixed inset-0 flex items-center justify-center ${
      isDark
        ? "bg-gradient-to-br from-slate-950 via-slate-900 to-slate-950"
        : "bg-gradient-to-br from-slate-50 via-white to-slate-100"
    }`}>
      <form
        onSubmit={handleSubmit}
        className={`w-full max-w-sm mx-4 p-6 rounded-2xl shadow-xl border ${
          isDark ? "bg-slate-800/80 border-slate-700" : "bg-white border-slate-200"
        }`}
      >
        <div className="flex flex-col items-center gap-1 mb-6">
          <h1 className={`text-lg font-semibold ${isDark ? "text-slate-100" : "text-slate-800"}`}>
            CCCC
          </h1>
          <p className={`text-sm ${isDark ? "text-slate-400" : "text-slate-500"}`}>
            {t('enterToken')}
          </p>
          <p className={`text-xs text-center ${isDark ? "text-slate-500" : "text-slate-500"}`}>
            {t('tokenLoginHint')}
          </p>
        </div>
        <div className="relative">
          <input
            type={showToken ? "text" : "password"}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder={t('accessToken')}
            autoFocus
            className={`w-full px-4 py-2.5 pr-20 rounded-lg border text-sm outline-none transition-colors ${
              isDark
                ? "bg-slate-700 border-slate-600 text-slate-100 placeholder-slate-400 focus:border-cyan-500"
                : "bg-slate-50 border-slate-300 text-slate-800 placeholder-slate-400 focus:border-cyan-500"
            }`}
          />
          <button
            type="button"
            onClick={() => setShowToken((prev) => !prev)}
            className={`absolute right-2 top-1/2 -translate-y-1/2 px-2 py-1 rounded text-xs transition-colors ${
              isDark ? "text-slate-300 hover:bg-slate-600" : "text-slate-600 hover:bg-slate-200"
            }`}
          >
            {showToken ? t('hideToken') : t('showToken')}
          </button>
        </div>
        {error && <p className="mt-2 text-sm text-red-400">{error}</p>}
        <button
          type="submit"
          disabled={submitting || !token.trim()}
          className={`w-full mt-4 px-4 py-2.5 rounded-lg text-sm font-medium transition-colors ${
            submitting || !token.trim() ? "opacity-50 cursor-not-allowed" : ""
          } ${
            isDark
              ? "bg-cyan-600 hover:bg-cyan-500 text-white"
              : "bg-cyan-500 hover:bg-cyan-600 text-white"
          }`}
        >
          {submitting ? t('verifying') : t('signIn')}
        </button>
      </form>
    </div>
  );
}
