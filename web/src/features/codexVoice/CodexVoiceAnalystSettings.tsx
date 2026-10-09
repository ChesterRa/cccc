import { useTranslation } from "react-i18next";
import { StructuredRuntimeSettingsFields } from "../../components/modals/StructuredRuntimeSettingsFields";
import { Button } from "../../components/ui/button";
import { VoiceRuntimeProfileActions } from "../voice/VoiceRuntimeProfileActions";
import { useCodexVoiceAnalystSettings } from "./useCodexVoiceAnalystSettings";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function CodexVoiceAnalystSettings({
  active,
  controller,
}: {
  active: boolean;
  controller: CodexVoiceSessionController;
}) {
  const form = useCodexVoiceAnalystSettings(active, controller);
  return <CodexVoiceAnalystSettingsFields form={form} controller={controller} />;
}

export function CodexVoiceAnalystSettingsFields({
  controller,
  form,
  onProfileEditingChange,
}: {
  form: ReturnType<typeof useCodexVoiceAnalystSettings>;
  controller: CodexVoiceSessionController;
  onProfileEditingChange?: (editing: boolean) => void;
}) {
  const { t } = useTranslation("modals");

  return (
    <section
      className="min-w-0 space-y-5"
      aria-busy={form.loading || form.saving || form.profileSaving}
    >
      <StructuredRuntimeSettingsFields
        form={form}
        runtimeHint={t("codexVoiceAnalystRuntimeHint")}
        commandHint={t("codexVoiceAnalystCommandHint")}
      />

      <div className="space-y-3 border-t border-[var(--glass-border-subtle)] pt-4">
        <div className="min-w-0 text-xs leading-5 break-words">
          {form.hasChanges ? (
            <p className="mb-1 font-medium text-[var(--color-text-primary)]">
              {t("codexVoiceUnsavedChanges")}
            </p>
          ) : null}
          {form.error ? (
            <p className="text-rose-500" role="alert">
              {form.error}
            </p>
          ) : form.callActive ? (
            <p className="text-amber-700 dark:text-amber-300">
              {t("codexVoiceAnalystSettingsCallActive")}
            </p>
          ) : form.analystBusy ? (
            <p className="text-amber-700 dark:text-amber-300">
              {t("codexVoiceAnalystSettingsWorkActive")}
            </p>
          ) : form.saved ? (
            <p className="text-emerald-600 dark:text-emerald-400" role="status">
              {form.saved}
            </p>
          ) : (
            <p className="text-[var(--color-text-muted)]">
              {t("codexVoiceAnalystSettingsApplyHint")}
            </p>
          )}
        </div>
        <div className="flex flex-wrap justify-end gap-2 [&>button]:min-w-0 [&>button]:whitespace-normal">
          {form.hasChanges ? (
            <Button
              type="button"
              variant="ghost"
              disabled={form.editingDisabled}
              onClick={form.discard}
            >
              {t("codexVoiceDiscardChanges")}
            </Button>
          ) : null}
          <VoiceRuntimeProfileActions
            form={form}
            name="Voice Analyst"
            onEditingChange={onProfileEditingChange}
          />
          <Button
            type="button"
            className="w-full sm:w-auto"
            onClick={() => void form.save()}
            disabled={form.saveDisabled}
          >
            {form.saving
              ? t("codexVoiceAnalystSettingsSaving")
              : controller.analyst
                ? t("codexVoiceAnalystSettingsApplyRestart")
                : t("codexVoiceAnalystSettingsSave")}
          </Button>
        </div>
      </div>
    </section>
  );
}
