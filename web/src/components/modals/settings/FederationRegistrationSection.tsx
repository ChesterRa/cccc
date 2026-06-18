import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type { FederationRegistration } from "../../../services/api/federation";
import type { GroupMeta } from "../../../types";
import { FederationHttpRegistrationSection } from "./FederationHttpRegistrationSection";
import { settingsWorkspaceHeaderClass, settingsWorkspaceShellClass, settingsWorkspaceSoftPanelClass } from "./types";

interface FederationRegistrationSectionProps {
  isDark: boolean;
  isActive?: boolean;
}

export function FederationRegistrationSection({ isDark, isActive }: FederationRegistrationSectionProps) {
  const { t } = useTranslation("settings");
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [registrations, setRegistrations] = useState<FederationRegistration[]>([]);

  const refreshStatus = useCallback(async () => {
    const resp = await api.fetchFederationStatus();
    if (resp.ok) setRegistrations(resp.result.registrations || []);
  }, []);

  useEffect(() => {
    if (isActive === false) return undefined;
    let cancelled = false;
    void api.fetchGroups().then((resp) => {
      if (!cancelled && resp.ok) setGroups(resp.result.groups || []);
    });
    void api.fetchFederationStatus().then((resp) => {
      if (!cancelled && resp.ok) setRegistrations(resp.result.registrations || []);
    });
    return () => {
      cancelled = true;
    };
  }, [isActive]);

  return (
    <div className="space-y-5">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("federation.title")}</h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">{t("federation.globalLead")}</p>
          </div>
        </div>
        <div className="px-4 pt-4 sm:px-5">
          <div className={settingsWorkspaceSoftPanelClass(isDark)}>{t("federation.libp2pManagedPerGroup")}</div>
        </div>
        <FederationHttpRegistrationSection
          isDark={isDark}
          groups={groups}
          registrations={registrations}
          refreshStatus={refreshStatus}
        />
      </div>
    </div>
  );
}

export default FederationRegistrationSection;
