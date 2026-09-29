import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUIStore, type SSEErrorDetail } from "../stores/useUIStore";

/**
 * Reads a transport `retry` event: the shared socket dropped ("socket") or
 * the server closed one subscription on a healthy socket ("channel").
 */
export function readRetryDetail(event: Event, now = Date.now()): SSEErrorDetail | null {
  try {
    const data = JSON.parse(String((event as MessageEvent).data || "{}"));
    const delayMs = Number(data.delay_ms);
    // Zero means the retry has already started (the reconnect handshake).
    if (!Number.isFinite(delayMs) || delayMs < 0) return null;
    return { cause: data.cause === "channel" ? "channel" : "socket", nextRetryAt: now + delayMs };
  } catch {
    return null;
  }
}

/**
 * Composes the "why" for an interrupted stream and the live retry countdown,
 * e.g. "realtime connection unreachable · retrying in 7s". Empty while
 * connected.
 */
export function useSseErrorDetailText(): string {
  const { t } = useTranslation("layout");
  const sseError = useUIStore((s) => s.sseError);
  const nextRetryAt = sseError?.nextRetryAt ?? null;

  // The remaining time is read from the clock during render, so the first
  // frame after a retry is scheduled already shows the true remainder. The
  // timer only wakes when the rounded-up second changes and stops at the
  // deadline, so a stalled reconnect does not keep re-rendering the header.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (nextRetryAt === null) return;
    let timer = 0;
    const schedule = () => {
      const remaining = nextRetryAt - Date.now();
      if (remaining <= 0) return;
      timer = window.setTimeout(
        () => {
          setTick((tick) => tick + 1);
          schedule();
        },
        remaining % 1000 || 1000,
      );
    };
    schedule();
    return () => window.clearTimeout(timer);
  }, [nextRetryAt]);

  if (!sseError) return "";
  const seconds = Math.ceil((sseError.nextRetryAt - Date.now()) / 1000);
  return [
    t(sseError.cause === "channel" ? "realtimeSubscriptionClosed" : "realtimeSocketUnreachable"),
    seconds > 0 ? t("retryingIn", { seconds }) : t("retryingNow"),
  ].join(" · ");
}
