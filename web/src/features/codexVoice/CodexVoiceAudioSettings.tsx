import { useTranslation } from "react-i18next";
import { SelectCombobox } from "../../components/SelectCombobox";
import { inputClass } from "../../components/modals/settings/types";
import { SettingsCard, SettingsRow } from "../../components/modals/settings/SettingsKit";
import { formatCodexVoiceName, type CodexRealtimeVoice } from "./codexVoicePreferences";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function CodexVoiceAudioSettings({
  controller,
}: {
  controller: CodexVoiceSessionController;
}) {
  const { t } = useTranslation("modals");
  return (
    <SettingsCard
      title={t("codexVoiceSettingsVoiceAudio")}
      description={t("codexVoiceAudioPreferencesHint")}
    >
      <SettingsRow label={t("codexVoiceVoiceLabel")}>
        <SelectCombobox
          ariaLabel={t("codexVoiceVoiceLabel")}
          className={inputClass()}
          value={controller.preferences.voice}
          onChange={(voice) => controller.updatePreferences({ voice: voice as CodexRealtimeVoice })}
          items={controller.supportedVoices.map((voice) => ({
            value: voice,
            label: formatCodexVoiceName(voice),
          }))}
        />
      </SettingsRow>
    </SettingsCard>
  );
}
