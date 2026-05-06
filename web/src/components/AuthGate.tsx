import React, { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from 'react-i18next';
import { useTheme } from "../hooks/useTheme";
import * as api from "../services/api";
import { useBrandingStore } from "../stores";
import { resolveThemeAwareLogoUrl } from "../utils/branding";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Surface } from "./ui/surface";

type AuthStatus = "checking" | "authenticated" | "login";

function needsTokenLogin(resp: api.ApiResponse<unknown>): boolean {
  return !resp.ok && (resp.error?.code === "unauthorized" || resp.error?.code === "permission_denied");
}

export function AuthGate({ children }: { children: React.ReactNode }) {
  const { isDark } = useTheme();
  const initialForceLogin = api.shouldForceTokenLogin();
  const forceLoginRef = useRef(initialForceLogin);
  const [status, setStatus] = useState<AuthStatus>(initialForceLogin ? "login" : "checking");
  const [token, setToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);
  const { t } = useTranslation('layout');
  const branding = useBrandingStore((s) => s.branding);
  const logoSrc = resolveThemeAwareLogoUrl(branding.logo_icon_url, isDark);
  const hostname = typeof window !== "undefined" ? String(window.location.hostname || "").trim().toLowerCase() : "";
  const isLocalAccess = hostname === "localhost" || hostname === "127.0.0.1" || hostname === "::1" || hostname === "[::1]";
  const localRecoveryPath = "~/.cccc/access_tokens.yaml";

  // Probe a protected endpoint on startup; 401/403 means token auth is enabled
  // and this browser is not authenticated yet.
  useEffect(() => {
    if (forceLoginRef.current) {
      api.clearAuthToken();
      return;
    }
    let cancelled = false;
    api.fetchGroups().then((resp) => {
      if (cancelled) return;
      if (resp.ok) {
        setStatus("authenticated");
      } else if (needsTokenLogin(resp)) {
        api.clearAuthToken();
        setStatus("login");
      } else {
        // Do not block startup for service reachability or other app-level errors.
        setStatus("authenticated");
      }
    });
    return () => {
      cancelled = true;
    };
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
      api.clearForceTokenLogin();
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
      <div className="fixed inset-0 flex items-center justify-center bg-[var(--color-bg-primary)]">
        <div className="text-sm text-[var(--color-text-tertiary)]">
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
    <div className="fixed inset-0 flex items-center justify-center bg-[var(--color-bg-primary)]">
      <form
        onSubmit={handleSubmit}
        className="glass-modal w-full max-w-sm mx-4 p-6"
      >
        <div className="flex flex-col items-center gap-1 mb-6">
          <div className="mb-2 flex h-12 min-w-[48px] max-w-[220px] items-center justify-center overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 shadow-sm">
            <img
              src={logoSrc}
              alt={`${branding.product_name} logo`}
              className="max-h-7 w-auto max-w-full object-contain"
            />
          </div>
          <h1 className="text-lg font-semibold gradient-text">
            {branding.product_name}
          </h1>
          <p className="text-sm text-[var(--color-text-tertiary)]">
            {t('enterToken')}
          </p>
          <p className="text-xs text-center text-[var(--color-text-muted)]">
            {t('tokenLoginHint')}
          </p>
        </div>
        <div className="relative">
          <Input
            type={showToken ? "text" : "password"}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder={t('accessToken')}
            autoFocus
            className="pr-20"
          />
          <button
            type="button"
            onClick={() => setShowToken((prev) => !prev)}
            className="absolute right-2 top-1/2 -translate-y-1/2 px-2 py-1 rounded text-xs text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] transition-colors cursor-pointer"
          >
            {showToken ? t('hideToken') : t('showToken')}
          </button>
        </div>
        {error && <p className="mt-2 text-sm text-red-400">{error}</p>}
        <Button
          type="submit"
          disabled={submitting || !token.trim()}
          className="mt-4 w-full disabled:opacity-70"
        >
          {submitting ? t('verifying') : t('signIn')}
        </Button>

        <div className="mt-4 border-t pt-4 border-[var(--glass-border-subtle)]">
          <button
            type="button"
            onClick={() => setShowRecovery((prev) => !prev)}
            className="text-xs underline underline-offset-4 transition-colors text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]"
          >
            {showRecovery ? t('hideRecovery') : t('forgotTokenCta')}
          </button>

          {showRecovery ? (
            <Surface className="mt-3 text-left" radius="md">
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t('recoveryTitle')}</div>
              <p className="mt-2 text-xs leading-6 text-[var(--color-text-secondary)]">{t('recoveryIntro')}</p>

              <div className="mt-3 space-y-3">
                <div>
                  <div className="text-xs font-semibold text-[var(--color-text-primary)]">{t('recoveryBrowserTitle')}</div>
                  <p className="mt-1 text-xs leading-6 text-[var(--color-text-tertiary)]">{t('recoveryBrowserBody')}</p>
                </div>

                {isLocalAccess ? (
                  <div>
                    <div className="text-xs font-semibold text-[var(--color-text-primary)]">{t('recoveryLocalTitle')}</div>
                    <ol className="mt-1 list-decimal space-y-1 pl-4 text-xs leading-6 text-[var(--color-text-tertiary)]">
                      <li>{t('recoveryLocalStep1')}</li>
                      <li>{t('recoveryLocalStep2', { path: localRecoveryPath })}</li>
                      <li>{t('recoveryLocalStep3')}</li>
                      <li>{t('recoveryLocalStep4')}</li>
                    </ol>
                  </div>
                ) : (
                  <div>
                    <div className="text-xs font-semibold text-[var(--color-text-primary)]">{t('recoveryRemoteTitle')}</div>
                    <p className="mt-1 text-xs leading-6 text-[var(--color-text-tertiary)]">{t('recoveryRemoteBody', { path: localRecoveryPath })}</p>
                  </div>
                )}
              </div>

              <p className="mt-3 text-[11px] leading-5 text-[var(--color-text-muted)]">{t('recoverySecurityNote')}</p>
            </Surface>
          ) : null}
        </div>
      </form>
    </div>
  );
}
