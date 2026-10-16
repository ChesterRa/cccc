import { useTranslation } from "react-i18next";
import { SelectMenu } from "../../components/ui/select-menu";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function VoiceProviderSelect({
  controller,
  className,
}: {
  controller: CodexVoiceSessionController;
  className?: string;
}) {
  const { t } = useTranslation("modals");
  return (
    <SelectMenu
      ariaLabel={t("codexVoiceProviderLabel")}
      className={className}
      contentClassName="min-w-52"
      align="start"
      value={controller.call?.provider || controller.preferences?.provider || "codex"}
      disabled={controller.isEngaged}
      options={(["codex", "grok"] as const).map((value) => ({
        value,
        label: t(`codexVoiceProviders.${value}`),
      }))}
      onChange={(provider) => {
        if (provider !== "codex" && provider !== "grok") return;
        controller.updatePreferences({ provider });
        controller.clearError();
      }}
    />
  );
}
