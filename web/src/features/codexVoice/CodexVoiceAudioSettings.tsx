import { useTranslation } from "react-i18next";
import { inputClass } from "../../components/modals/settings/types";
import { SettingsCard, SettingsRow } from "../../components/modals/settings/SettingsKit";
import { SpeakingVoiceSelect } from "./SpeakingVoiceSelect";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { VoiceProviderSelect } from "./VoiceProviderSelect";

export function CodexVoiceAudioSettings({
  controller,
}: {
  controller: CodexVoiceSessionController;
}) {
  const { t } = useTranslation("modals");
  return (
    <SettingsCard
      title={t("codexVoiceSettingsVoiceAudio")}
      description={`${t("codexVoiceExperimental")} · ${t("codexVoiceAudioPreferencesHint")}`}
    >
      <SettingsRow label={t("codexVoiceProviderLabel")}>
        <VoiceProviderSelect controller={controller} className={inputClass()} />
      </SettingsRow>
      <SettingsRow label={t("codexVoiceVoiceLabel")}>
        <SpeakingVoiceSelect controller={controller} className={inputClass()} />
      </SettingsRow>
    </SettingsCard>
  );
}
