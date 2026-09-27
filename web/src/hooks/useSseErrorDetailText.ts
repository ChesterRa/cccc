import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUIStore } from "../stores/useUIStore";

/**
 * Composes the "why" for an interrupted stream: which connection failed and
 * the live retry countdown, e.g. "realtime socket (global, ledger)
 * unreachable · retrying in 7s". Empty while connected.
 */
export function useSseErrorDetailText(): string {
  const { t } = useTranslation("layout");
  const sseError = useUIStore((s) => s.sseError);
  const nextRetryAt = sseError?.nextRetryAt ?? null;

  // The remaining time is read from the clock during render, so the first
  // frame after a retry is scheduled already shows the true remainder. The
  // interval only asks React to render again while a deadline is pending.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!nextRetryAt) return;
    const timer = window.setInterval(() => setTick((tick) => tick + 1), 250);
    return () => window.clearInterval(timer);
  }, [nextRetryAt]);

  if (!sseError) return "";
  const reason = `${sseError.endpoint} ${t("connectionUnreachable")}`;
  const countdown = nextRetryAt
    ? t("retryingIn", { seconds: Math.max(0, Math.ceil((nextRetryAt - Date.now()) / 1000)) })
    : "";
  return [reason, countdown].filter(Boolean).join(" · ");
}
