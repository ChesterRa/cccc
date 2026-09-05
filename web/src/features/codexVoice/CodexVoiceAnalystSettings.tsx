import { useTranslation } from "react-i18next";
import { ActorSecretManager } from "../../components/modals/ActorSecretManager";
import {
  OpenCodeManagedModelHint,
  RuntimeCommandControl,
  RuntimeConfigurationModePicker,
  RuntimeProfilePicker,
} from "../../components/modals/RuntimeProfileControls";
import { SelectCombobox } from "../../components/SelectCombobox";
import { Button } from "../../components/ui/button";
import { RUNTIME_INFO } from "../../types";
import { useCodexVoiceAnalystSettings } from "./useCodexVoiceAnalystSettings";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function CodexVoiceAnalystSettings({
  active,
  controller,
}: {
  active: boolean;
  controller: CodexVoiceSessionController;
}) {
  const { t } = useTranslation("modals");
  const { t: tActors } = useTranslation("actors");
  const form = useCodexVoiceAnalystSettings(active, controller);

  return (
    <section
      className="flex flex-col"
      aria-busy={form.loading || form.saving || form.profileSaving}
    >
      <div className="space-y-5 px-5 py-5 sm:px-6 sm:py-6">
        <div>
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
            {tActors("sectionRuntime")}
          </h3>
          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
            {t("codexVoiceAnalystRuntimeHint")}
          </p>

          <div className="mt-4">
            <RuntimeConfigurationModePicker
              value={form.mode}
              disabled={form.editingDisabled}
              onChange={form.changeMode}
            />
          </div>

          <div className="mt-4">
            {form.mode === "profile" ? (
              <RuntimeProfilePicker
                value={form.profileIdentity}
                profiles={form.compatibleProfiles}
                busy={form.loading}
                disabled={form.editingDisabled}
                emptyHint={t("codexVoiceAnalystCompatibleProfilesEmpty")}
                hostNote={t("codexVoiceAnalystProfileHostNote")}
                detailsLabel={t("codexVoiceAnalystProfileDetails")}
                onChange={form.selectProfile}
              />
            ) : (
              <div className="space-y-4">
                <div>
                  <label className="mb-2 block text-xs font-medium text-[var(--color-text-muted)]">
                    {tActors("runtime")}
                  </label>
                  <SelectCombobox
                    className="w-full min-h-[44px] rounded-xl border px-4 py-2.5 text-sm glass-input text-[var(--color-text-primary)]"
                    value={form.settings.runtime}
                    onChange={form.setRuntime}
                    disabled={form.editingDisabled}
                    ariaLabel={tActors("runtime")}
                    items={[
                      { value: "codex", label: RUNTIME_INFO.codex.label },
                      { value: "claude", label: RUNTIME_INFO.claude.label },
                      { value: "grok", label: RUNTIME_INFO.grok.label },
                      { value: "opencode", label: RUNTIME_INFO.opencode.label },
                      { value: "kilo", label: RUNTIME_INFO.kilo.label },
                    ]}
                  />
                  <p className="mt-1.5 text-[10px] leading-4 text-[var(--color-text-muted)]">
                    {t("codexVoiceAnalystSupportedRuntimesHint")}
                  </p>
                  <OpenCodeManagedModelHint runtime={form.settings.runtime} />
                </div>

                <RuntimeCommandControl
                  runtime={form.settings.runtime}
                  command={form.settings.command}
                  defaultCommand={form.defaultCommand}
                  useDefaultCommand={form.useDefaultCommand}
                  disabled={form.editingDisabled}
                  description={t("codexVoiceAnalystCommandHint")}
                  onCommandChange={form.setCommand}
                  onUseDefaultCommandChange={form.setUseDefaultCommand}
                />
              </div>
            )}
          </div>
        </div>

        {form.mode === "custom" ? (
          <details className="border-t border-[var(--glass-border-subtle)] pt-4">
            <summary className="cursor-pointer select-none text-sm font-semibold text-[var(--color-text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45">
              {tActors("sectionAdvanced")}
            </summary>
            <p className="ml-4 mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
              {tActors("sectionAdvancedHint")}
            </p>
            <div className="ml-4 mt-4 border-t border-[var(--glass-border-subtle)] pt-4">
              <div className="text-xs font-medium text-[var(--color-text-secondary)]">
                {tActors("secretsSection")}
              </div>
              <ActorSecretManager
                keys={form.environmentKeys}
                masks={{}}
                changes={form.environmentChanges}
                loading={form.environmentRefreshing}
                keysLoadFailed={form.settingsLoadFailed}
                disabled={form.editingDisabled}
                onRefresh={() => void form.refreshEnvironment()}
                onChangesChange={form.setEnvironmentChanges}
              />
            </div>
          </details>
        ) : null}
      </div>

      <div className="sticky bottom-0 mt-auto flex flex-col gap-3 border-t border-[var(--glass-border-subtle)] bg-[var(--color-sidebar-bg)] px-5 pt-4 pb-[calc(1rem+env(safe-area-inset-bottom,0px))] backdrop-blur-xl sm:flex-row sm:items-center sm:justify-between sm:px-6">
        <div className="min-w-0 text-xs">
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
        <div className="flex flex-col gap-2 sm:flex-row">
          {form.mode === "custom" ? (
            <Button
              type="button"
              variant="outline"
              onClick={() => void form.saveAsProfile()}
              disabled={form.editingDisabled || form.settingsLoadFailed}
            >
              {form.profileSaving
                ? t("codexVoiceAnalystProfileSaving")
                : tActors("addToActorProfiles")}
            </Button>
          ) : null}
          <Button type="button" onClick={() => void form.save()} disabled={form.saveDisabled}>
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
