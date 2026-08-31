import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { useTheme } from "../../hooks/useTheme";
import { useBrandingStore } from "../../stores";
import { resolveThemeAwareLogoUrl } from "../../utils/branding";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Surface } from "../ui/surface";

type AuthTokenLoginFormProps = {
  error: string;
  submitting: boolean;
  onSubmit: (token: string) => void | Promise<void>;
};

export function AuthTokenLoginForm({ error, submitting, onSubmit }: AuthTokenLoginFormProps) {
  const { t } = useTranslation("layout");
  const { isDark } = useTheme();
  const branding = useBrandingStore((state) => state.branding);
  const [token, setToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);
  const hostname = String(window.location.hostname || "")
    .trim()
    .toLowerCase();
  const isLocal = ["localhost", "127.0.0.1", "::1", "[::1]"].includes(hostname);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (token.trim()) void onSubmit(token);
  };

  return (
    <div
      data-testid="auth-login-scroll"
      className="fixed inset-0 flex overflow-y-auto bg-[var(--color-bg-primary)] px-4 pb-[max(1rem,env(safe-area-inset-bottom))] pt-[max(1rem,env(safe-area-inset-top))]"
    >
      <form onSubmit={submit} className="glass-modal m-auto w-full max-w-sm p-6">
        <div className="mb-6 flex flex-col items-center gap-1">
          <div className="mb-2 flex h-12 min-w-[48px] max-w-[220px] items-center justify-center overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 shadow-sm">
            <img
              src={resolveThemeAwareLogoUrl(branding.logo_icon_url, isDark)}
              alt={`${branding.product_name} logo`}
              className="max-h-7 w-auto max-w-full object-contain"
            />
          </div>
          <h1 className="gradient-text text-lg font-semibold">{branding.product_name}</h1>
          <p className="text-sm text-[var(--color-text-tertiary)]">{t("enterToken")}</p>
          <p className="text-center text-xs text-[var(--color-text-muted)]">
            {t("tokenLoginHint")}
          </p>
        </div>
        <div className="relative">
          <Input
            name="cccc-access-token"
            type={showToken ? "text" : "password"}
            value={token}
            onChange={(event) => setToken(event.target.value)}
            placeholder={t("accessToken")}
            aria-label={t("accessToken")}
            autoComplete="current-password"
            autoFocus
            className="pr-20"
          />
          <button
            type="button"
            onClick={() => setShowToken((visible) => !visible)}
            className="absolute right-2 top-1/2 -translate-y-1/2 cursor-pointer rounded px-2 py-1 text-xs text-[var(--color-text-secondary)] transition-colors hover:text-[var(--color-text-primary)]"
          >
            {showToken ? t("hideToken") : t("showToken")}
          </button>
        </div>
        {error ? <p className="mt-2 text-sm text-red-400">{error}</p> : null}
        <Button
          type="submit"
          disabled={submitting || !token.trim()}
          className="mt-4 w-full disabled:opacity-70"
        >
          {submitting ? t("verifying") : t("signIn")}
        </Button>
        <div className="mt-4 border-t border-[var(--glass-border-subtle)] pt-4">
          <button
            type="button"
            onClick={() => setShowRecovery((visible) => !visible)}
            className="text-xs text-[var(--color-text-secondary)] underline underline-offset-4 transition-colors hover:text-[var(--color-text-primary)]"
          >
            {showRecovery ? t("hideRecovery") : t("forgotTokenCta")}
          </button>
          {showRecovery ? <RecoveryHelp isLocal={isLocal} /> : null}
        </div>
      </form>
    </div>
  );
}

function RecoveryHelp({ isLocal }: { isLocal: boolean }) {
  const { t } = useTranslation("layout");
  const tokenPath = "~/.cccc/access_tokens.yaml";
  const bootstrapPath = "~/.cccc/web_bootstrap_token";
  return (
    <Surface className="mt-3 text-left" radius="md">
      <div className="text-sm font-semibold text-[var(--color-text-primary)]">
        {t("recoveryTitle")}
      </div>
      <p className="mt-2 text-xs leading-6 text-[var(--color-text-secondary)]">
        {t("recoveryIntro")}
      </p>
      <div className="mt-3 text-xs leading-6 text-[var(--color-text-tertiary)]">
        <div className="font-semibold text-[var(--color-text-primary)]">
          {t("recoveryBrowserTitle")}
        </div>
        <p>{t("recoveryBrowserBody")}</p>
        <div className="mt-3 font-semibold text-[var(--color-text-primary)]">
          {t(isLocal ? "recoveryLocalTitle" : "recoveryRemoteTitle")}
        </div>
        {isLocal ? (
          <ol className="list-decimal space-y-1 pl-4">
            <li>{t("recoveryLocalStep1")}</li>
            <li>{t("recoveryLocalStep2", { path: tokenPath })}</li>
            <li>{t("recoveryLocalStep3")}</li>
            <li>{t("recoveryLocalStep4", { bootstrapPath })}</li>
          </ol>
        ) : (
          <p>{t("recoveryRemoteBody", { path: tokenPath, bootstrapPath })}</p>
        )}
      </div>
      <p className="mt-3 text-[11px] leading-5 text-[var(--color-text-muted)]">
        {t("recoverySecurityNote")}
      </p>
    </Surface>
  );
}
