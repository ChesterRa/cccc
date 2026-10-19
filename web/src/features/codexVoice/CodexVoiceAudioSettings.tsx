import { useTranslation } from "react-i18next";
import { SelectCombobox } from "../../components/SelectCombobox";
import { inputClass } from "../../components/modals/settings/types";
import { SettingsCard, SettingsRow } from "../../components/modals/settings/SettingsKit";
import { codexVoicePrimaryLanguage, formatCodexVoiceName } from "./codexVoicePreferences";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { VoiceProviderSelect } from "./VoiceProviderSelect";

export function CodexVoiceAudioSettings({
  controller,
}: {
  controller: CodexVoiceSessionController;
}) {
  const { t } = useTranslation("modals");
  const provider = controller.call?.provider || controller.preferences.provider || "codex";
  return (
    <SettingsCard
      title={t("codexVoiceSettingsVoiceAudio")}
      description={`${t("codexVoiceExperimental")} · ${t("codexVoiceAudioPreferencesHint")}`}
    >
      <SettingsRow label={t("codexVoiceProviderLabel")}>
        <VoiceProviderSelect controller={controller} className={inputClass()} />
      </SettingsRow>
      <SettingsRow label={t("codexVoiceVoiceLabel")}>
        <SelectCombobox
          ariaLabel={t("codexVoiceVoiceLabel")}
          className={inputClass()}
          contentClassName="min-w-[min(20rem,calc(100vw-24px))]"
          value={controller.call?.voice || controller.preferences.voice}
          placeholder={formatCodexVoiceName(controller.call?.voice || controller.preferences.voice)}
          disabled={controller.isEngaged || controller.supportedVoices.length === 0}
          searchable
          onChange={(voice) => controller.updatePreferences({ voice })}
          items={controller.supportedVoices.map((voice) => {
            const language = provider === "codex" ? codexVoicePrimaryLanguage(voice) : undefined;
            return {
              value: voice,
              label: formatCodexVoiceName(voice),
              ...(language
                ? {
                    description: t("codexVoicePrimaryLanguage", {
                      language: t(`codexVoiceLanguages.${language}`),
                    }),
                    keywords: [language],
                  }
                : {}),
            };
          })}
        />
      </SettingsRow>
    </SettingsCard>
  );
}
