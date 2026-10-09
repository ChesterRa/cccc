import { useTranslation } from "react-i18next";
import { SelectCombobox } from "../../components/SelectCombobox";
import { inputClass, labelClass } from "../../components/modals/settings/types";
import { formatCodexVoiceName, type CodexRealtimeVoice } from "./codexVoicePreferences";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function CodexVoiceAudioSettings({
  controller,
}: {
  controller: CodexVoiceSessionController;
}) {
  const { t } = useTranslation("modals");
  return (
    <section className="min-w-0 space-y-4" aria-labelledby="codex-voice-audio-heading">
      <div>
        <h3 id="codex-voice-audio-heading" className="text-sm font-semibold">
          {t("codexVoiceVoiceLabel")}
        </h3>
        <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
          {t("codexVoiceAudioPreferencesHint")}
        </p>
      </div>
      <label className="block min-w-0 max-w-lg">
        <span className={labelClass()}>{t("codexVoiceVoiceLabel")}</span>
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
      </label>
    </section>
  );
}
