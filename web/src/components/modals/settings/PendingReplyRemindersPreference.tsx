import { useId } from "react";
import { useTranslation } from "react-i18next";

import { useUIStore } from "../../../stores/useUIStore";
import { SettingsCard, SettingsRow, SettingsSwitch } from "./SettingsKit";

export function PendingReplyRemindersPreference() {
  const { t } = useTranslation("settings");
  const labelId = useId();
  const showReminders = useUIStore((state) => state.showPendingReplyReminders);
  const setShowReminders = useUIStore((state) => state.setShowPendingReplyReminders);

  return (
    <SettingsCard
      title={t("messaging.displayPreferences")}
      description={t("messaging.pendingReplyRemindersScope")}
    >
      <SettingsRow
        label={t("messaging.showPendingReplyReminders")}
        help={t("messaging.pendingReplyRemindersHelp")}
        labelId={labelId}
      >
        <SettingsSwitch checked={showReminders} onChange={setShowReminders} labelledBy={labelId} />
      </SettingsRow>
    </SettingsCard>
  );
}
