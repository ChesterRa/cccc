import { useTranslation } from "react-i18next";
import { SelectCombobox } from "../../components/SelectCombobox";
import { codexVoicePrimaryLanguage, formatCodexVoiceName } from "./codexVoicePreferences";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function SpeakingVoiceSelect({
  controller,
  className,
}: {
  controller: CodexVoiceSessionController;
  className?: string;
}) {
  const { t } = useTranslation("modals");
  const selection = controller.call || controller.preferences;
  const provider = selection.provider || "codex";
  return (
    <SelectCombobox
      ariaLabel={t("codexVoiceVoiceLabel")}
      className={className}
      contentClassName="min-w-[min(20rem,calc(100vw-24px))]"
      value={selection.voice}
      placeholder={formatCodexVoiceName(selection.voice)}
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
  );
}
