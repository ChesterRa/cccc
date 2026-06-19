import { useTranslation } from "react-i18next";

import { settingsWorkspaceHeaderClass, settingsWorkspaceShellClass, settingsWorkspaceSoftPanelClass } from "./types";

interface FederationRegistrationSectionProps {
  isDark: boolean;
  isActive?: boolean;
}

export function FederationRegistrationSection({ isDark, isActive }: FederationRegistrationSectionProps) {
  void isActive;
  const { t } = useTranslation("settings");

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
          <div className={settingsWorkspaceSoftPanelClass(isDark)}>{t("federation.sessionManagedPerGroup")}</div>
        </div>
      </div>
    </div>
  );
}

export default FederationRegistrationSection;
