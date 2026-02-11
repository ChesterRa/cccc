import React, { useEffect, useState, useCallback, useRef } from "react";
import { useTheme } from "../hooks/useTheme";
import * as api from "../services/api";

type AuthStatus = "checking" | "authenticated" | "login";

export function AuthGate({ children }: { children: React.ReactNode }) {
  const { isDark } = useTheme();
  const [status, setStatus] = useState<AuthStatus>("checking");
  const [token, setToken] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const calledRef = useRef(false);

  // On mount: probe /api/v1/ping to determine if auth is required.
  useEffect(() => {
    if (calledRef.current) return;
    calledRef.current = true;
    api.fetchPing().then((resp) => {
      if (resp.ok) {
        setStatus("authenticated");
      } else if (resp.error?.code === "unauthorized") {
        setStatus("login");
      } else {
        // Server unreachable or other error — let App handle it.
        setStatus("authenticated");
      }
    });
  }, []);

  // Subscribe to mid-session 401s so the gate re-appears.
  useEffect(() => {
    api.onAuthRequired(() => setStatus("login"));
  }, []);

  const handleSubmit = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    const t = token.trim();
    if (!t) return;
    setSubmitting(true);
    setError("");
    api.setAuthToken(t);
    const resp = await api.fetchPing();
    setSubmitting(false);
    if (resp.ok) {
      setStatus("authenticated");
    } else {
      api.clearAuthToken();
      setError(
        resp.error?.code === "unauthorized"
          ? "Token incorrect"
          : resp.error?.message || "Connection failed",
      );
    }
  }, [token]);

  if (status === "checking") {
    return (
      <div className={`fixed inset-0 flex items-center justify-center ${
        isDark ? "bg-slate-950" : "bg-slate-50"
      }`}>
        <div className={`text-sm ${isDark ? "text-slate-400" : "text-slate-500"}`}>
          Connecting...
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
            Enter access token to continue
          </p>
        </div>
        <input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder="Access Token"
          autoFocus
          className={`w-full px-4 py-2.5 rounded-lg border text-sm outline-none transition-colors ${
            isDark
              ? "bg-slate-700 border-slate-600 text-slate-100 placeholder-slate-400 focus:border-cyan-500"
              : "bg-slate-50 border-slate-300 text-slate-800 placeholder-slate-400 focus:border-cyan-500"
          }`}
        />
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
          {submitting ? "Verifying..." : "Sign In"}
        </button>
      </form>
    </div>
  );
}
