import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUIStore } from "../stores/useUIStore";

/**
 * Composes the "why" for an interrupted stream: which call failed, its HTTP
 * status or reachability, and the live retry countdown — e.g.
 * "ledger stream HTTP 404 · retrying in 7s". Empty while connected.
 */
export function useSseErrorDetailText(): string {
  const { t } = useTranslation("layout");
  const sseError = useUIStore((s) => s.sseError);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!sseError?.nextRetryAt) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [sseError?.nextRetryAt]);

  if (!sseError) return "";
  const reason = `${sseError.endpoint}${
    sseError.status ? ` HTTP ${sseError.status}` : ` ${t("connectionUnreachable")}`
  }`;
  const countdown = sseError.nextRetryAt
    ? t("retryingIn", {
        seconds: Math.max(0, Math.ceil((sseError.nextRetryAt - now) / 1000)),
      })
    : "";
  return [reason, countdown].filter(Boolean).join(" · ");
}
