import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import type { WebAccessSession } from "../../../types";

type SummaryTone = "neutral" | "good" | "warn";

export interface WebAccessSummary {
  label: string;
  detail: string;
  tone: SummaryTone;
}

export function useWebAccessSessionSummaries(
  knownAccessTokenCount: number,
  loginActive: boolean,
  session: WebAccessSession | null,
): { accessSummary: WebAccessSummary; currentBrowserSummary: WebAccessSummary } {
  const { t } = useTranslation("settings");

  const accessSummary = useMemo<WebAccessSummary>(() => {
    if (knownAccessTokenCount <= 0) {
      return {
        label: t("webAccess.summary.open"),
        detail: t("webAccess.summary.openHint"),
        tone: "neutral",
      };
    }
    return {
      label: t("webAccess.summary.protected"),
      detail: t("webAccess.summary.protectedHint", { count: knownAccessTokenCount }),
      tone: "good",
    };
  }, [knownAccessTokenCount, t]);

  const currentBrowserSummary = useMemo<WebAccessSummary>(() => {
    if (!loginActive) {
      return {
        label: t("webAccess.currentBrowserOpen"),
        detail: t("webAccess.currentBrowserOpenHint"),
        tone: "neutral",
      };
    }
    if (session == null) {
      return {
        label: t("webAccess.currentBrowserChecking"),
        detail: t("webAccess.currentBrowserCheckingHint"),
        tone: "warn",
      };
    }
    if (session.current_browser_signed_in) {
      return {
        label: t("webAccess.currentBrowserSignedIn"),
        detail: t("webAccess.currentBrowserSignedInHint", {
          userId: session.user_id || t("webAccess.unknownUser"),
          role: session.is_admin ? t("webAccess.adminBadge") : t("webAccess.scopedBadge"),
        }),
        tone: "good",
      };
    }
    return {
      label: t("webAccess.currentBrowserNotSignedIn"),
      detail: t("webAccess.currentBrowserNotSignedInHint"),
      tone: "warn",
    };
  }, [loginActive, session, t]);

  return { accessSummary, currentBrowserSummary };
}
