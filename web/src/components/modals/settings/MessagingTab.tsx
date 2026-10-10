import { useId } from "react";
import { useTranslation, Trans } from "react-i18next";

import { SegmentedChoice } from "../../ui/segmented-choice";
import { SettingsCard, SettingsPage, SettingsRow, SettingsSaveBar } from "./SettingsKit";

interface MessagingTabProps {
  isDark: boolean;
  busy: boolean;
  defaultSendTo: "foreman" | "broadcast";
  setDefaultSendTo: (v: "foreman" | "broadcast") => void;
  onSave: () => void;
}

export function MessagingTab(props: MessagingTabProps) {
  const { busy, defaultSendTo, setDefaultSendTo, onSave } = props;
  const { t } = useTranslation("settings");
  const labelId = useId();

  return (
    <SettingsPage
      title={t("messaging.title")}
      description={t("messaging.description")}
      footer={<SettingsSaveBar label={t("messaging.saveMessaging")} busy={busy} onSave={onSave} />}
    >
      <SettingsCard
        title={t("messaging.defaultRecipient")}
        description={
          <Trans
            i18nKey="messaging.defaultRecipientDescription"
            ns="settings"
            components={[<span key="recipient" className="font-mono" />]}
          />
        }
      >
        <SettingsRow
          label={t("messaging.whenNoRecipients")}
          help={t("messaging.tip")}
          labelId={labelId}
        >
          <SegmentedChoice<"foreman" | "broadcast">
            labelledBy={labelId}
            fill={false}
            value={defaultSendTo}
            onChange={setDefaultSendTo}
            options={[
              { value: "foreman", label: t("messaging.foremanOnly") },
              { value: "broadcast", label: t("messaging.broadcastAll") },
            ]}
          />
        </SettingsRow>
      </SettingsCard>
    </SettingsPage>
  );
}
