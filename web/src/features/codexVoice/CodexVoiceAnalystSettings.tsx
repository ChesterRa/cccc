import { useTranslation } from "react-i18next";
import { CodexVoiceSettingsHeading } from "./CodexVoiceSettingsSection";
import { SETTINGS_CARD, SETTINGS_COLUMN } from "./codexVoiceSettingsLayout";
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
  heading,
}: {
  active: boolean;
  controller: CodexVoiceSessionController;
  heading: string;
}) {
  const { t } = useTranslation("modals");
  const { t: tActors } = useTranslation("actors");
  const form = useCodexVoiceAnalystSettings(active, controller);

  return (
    <section
      className="flex min-h-full flex-1 flex-col"
      aria-busy={form.loading || form.saving || form.profileSaving}
    >
      <div
        className={`${SETTINGS_COLUMN} flex-1 space-y-4 py-4 @min-[900px]/voice-settings:space-y-5 @min-[900px]/voice-settings:py-10`}
      >
        <CodexVoiceSettingsHeading>{heading}</CodexVoiceSettingsHeading>
        <div className={`${SETTINGS_CARD} p-4 @min-[900px]/voice-settings:p-6`}>
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
              <div className="grid gap-4 @min-[900px]/voice-settings:grid-cols-2 @min-[900px]/voice-settings:gap-6">
                <div className="min-w-0">
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
                      { value: "antigravity", label: "Antigravity (ACP)" },
                      { value: "copilot", label: "GitHub Copilot (ACP)" },
                      { value: "devin", label: "Devin CLI (ACP)" },
                      { value: "cursor", label: "Cursor (ACP)" },
                    ]}
                  />
                  <p className="mt-1.5 text-[10px] leading-4 text-[var(--color-text-muted)]">
                    {t("codexVoiceAnalystSupportedRuntimesHint")}
                  </p>
                  <OpenCodeManagedModelHint runtime={form.settings.runtime} />
                  {form.settings.runtime === "antigravity" && (
                    <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">
                      {tActors("antigravityMode.hint")}
                      <br />
                      <code className="break-all">
                        cccc setup --runtime antigravity --runtime-mode acp --login
                      </code>
                    </p>
                  )}
                </div>

                <div className="min-w-0">
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
              </div>
            )}
          </div>
        </div>

        {form.mode === "custom" ? (
          <details className={`${SETTINGS_CARD} px-4 py-3`}>
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

      <div className="sticky bottom-0 z-10 border-t border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)]/90 pt-3 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))] backdrop-blur-xl">
        <div
          className={`${SETTINGS_COLUMN} flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between`}
        >
          <div className="min-w-0 flex-1 text-xs leading-5 break-words">
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
          <div className="flex flex-none flex-wrap justify-end gap-2">
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
      </div>
    </section>
  );
}
