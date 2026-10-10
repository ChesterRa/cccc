// DeliveryTab configures runtime delivery pacing and one-shot reminder windows.
import { useTranslation } from "react-i18next";

import {
  SettingsCard,
  SettingsNumberInput,
  SettingsPage,
  SettingsRow,
  SettingsSaveBar,
} from "./SettingsKit";

interface DeliveryTabProps {
  isDark: boolean;
  busy: boolean;
  mailNoticeAfterSeconds: number;
  setMailNoticeAfterSeconds: (v: number) => void;
  mailNoticeIdleAfterSeconds: number;
  setMailNoticeIdleAfterSeconds: (v: number) => void;
  replyNoticeAfterSeconds: number;
  setReplyNoticeAfterSeconds: (v: number) => void;
  onSave: () => void;
}

export function DeliveryTab(props: DeliveryTabProps) {
  const { busy, onSave } = props;
  const { t } = useTranslation("settings");
  const fields = [
    {
      id: "delivery-mail-notice",
      label: t("delivery.mailNotice"),
      help: t("delivery.mailNoticeHelp"),
      value: props.mailNoticeAfterSeconds,
      onChange: props.setMailNoticeAfterSeconds,
    },
    {
      id: "delivery-mail-idle-notice",
      label: t("delivery.mailIdleNotice"),
      help: t("delivery.mailIdleNoticeHelp"),
      value: props.mailNoticeIdleAfterSeconds,
      onChange: props.setMailNoticeIdleAfterSeconds,
    },
    {
      id: "delivery-reply-notice",
      label: t("delivery.replyNotice"),
      help: t("delivery.replyNoticeHelp"),
      value: props.replyNoticeAfterSeconds,
      onChange: props.setReplyNoticeAfterSeconds,
    },
  ];

  return (
    <SettingsPage
      title={t("delivery.title")}
      description={t("delivery.description")}
      footer={<SettingsSaveBar label={t("delivery.saveDelivery")} busy={busy} onSave={onSave} />}
    >
      <SettingsCard>
        {fields.map((field) => (
          <SettingsRow key={field.id} label={field.label} help={field.help} htmlFor={field.id}>
            <SettingsNumberInput
              id={field.id}
              value={field.value}
              onChange={field.onChange}
              showDuration
            />
          </SettingsRow>
        ))}
      </SettingsCard>
    </SettingsPage>
  );
}
