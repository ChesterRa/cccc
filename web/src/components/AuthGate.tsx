import React, { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../services/api";
import { AuthTokenLoginForm } from "./auth/AuthTokenLoginForm";

type AuthStatus = "checking" | "authenticated" | "login";

function needsTokenLogin(resp: api.ApiResponse<unknown>): boolean {
  return !resp.ok && api.isAuthRequiredErrorCode(resp.error?.code);
}

export function AuthGate({ children }: { children: React.ReactNode }) {
  const initialForceLogin = api.shouldForceTokenLogin();
  const forceLoginRef = useRef(initialForceLogin);
  const bootstrapRequiredRef = useRef(false);
  const [status, setStatus] = useState<AuthStatus>(initialForceLogin ? "login" : "checking");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const { t } = useTranslation("layout");

  // Establish the HttpOnly session cookie before opening any SSE/WebSocket.
  // Existing browsers may still hold only the header token in sessionStorage.
  useEffect(() => {
    if (forceLoginRef.current) {
      api.clearAuthToken();
      return;
    }
    let cancelled = false;
    void api.fetchWebAccessSession().then(async (session) => {
      const bootstrapRequired = Boolean(
        session.ok && session.result?.web_access_session?.bootstrap_required,
      );
      bootstrapRequiredRef.current = bootstrapRequired;
      if (bootstrapRequired) {
        if (!cancelled) setStatus("authenticated");
        return;
      }
      const resp = session.ok ? await api.fetchGroups() : session;
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
      if (bootstrapRequiredRef.current) return;
      api.clearAuthToken();
      setStatus("login");
    });
  }, []);

  const handleSubmit = useCallback(
    async (token: string) => {
      const trimmed = token.trim();
      if (!trimmed) return;
      setSubmitting(true);
      setError("");
      api.setAuthToken(trimmed);
      const session = await api.fetchWebAccessSession();
      if (!session.ok) {
        setSubmitting(false);
        api.clearAuthToken();
        setError(
          needsTokenLogin(session)
            ? t("tokenIncorrect")
            : session.error?.message || t("connectionFailed"),
        );
        return;
      }

      // Validate that Set-Cookie really took effect before discarding the bearer.
      api.clearAuthToken();
      const resp = await api.fetchGroups();
      setSubmitting(false);
      if (resp.ok) {
        api.clearForceTokenLogin();
        setStatus("authenticated");
      } else {
        api.setAuthToken(trimmed);
        setError(
          needsTokenLogin(resp)
            ? t("connectionFailed")
            : resp.error?.message || t("connectionFailed"),
        );
      }
    },
    [t],
  );

  if (status === "checking") {
    return (
      <div className="fixed inset-0 flex items-center justify-center bg-[var(--color-bg-primary)]">
        <div className="text-sm text-[var(--color-text-tertiary)]">{t("connecting")}</div>
      </div>
    );
  }

  if (status === "authenticated") {
    return <>{children}</>;
  }

  return <AuthTokenLoginForm error={error} submitting={submitting} onSubmit={handleSubmit} />;
}
