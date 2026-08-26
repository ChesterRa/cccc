import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";

export function useReachWebLogin(setError: (message: string) => void): () => Promise<string> {
  const { t } = useTranslation("settings");

  return useCallback(async (): Promise<string> => {
    try {
      const response = await api.createMembershipReachWebLogin();
      if (!response.ok) {
        setError(response.error?.message || t("webAccess.reach.adminAccessMissing"));
        return "";
      }
      const url = String(response.result?.web_url || "").trim();
      if (!url) {
        setError(t("webAccess.reach.adminAccessMissing"));
        return "";
      }
      setError("");
      return url;
    } catch {
      setError(t("webAccess.reach.adminAccessMissing"));
      return "";
    }
  }, [setError, t]);
}
