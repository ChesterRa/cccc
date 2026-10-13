import { useTranslation } from "react-i18next";
import { Button } from "../../ui/button";
import { SelectCombobox } from "../../SelectCombobox";
import { SettingsCard, SettingsRow, SettingsSaveBar, SettingsSwitch } from "./SettingsKit";
import { VoiceRuntimeProfileActions } from "../../../features/voice/VoiceRuntimeProfileActions";
import { secretaryReadinessIssue } from "../../../features/voice/secretaryReadiness";
import { StructuredRuntimeSettingsFields } from "../StructuredRuntimeSettingsFields";
import { LocalAsrModels } from "./LocalAsrModels";
import { ExternalAsrSettings } from "./ExternalAsrSettings";
import type { useVoiceSecretarySettings } from "./useVoiceSecretarySettings";
import { inputClass } from "./types";

export function VoiceSecretarySettings({
  active,
  visible,
  form,
  leaveError,
  onProfileEditingChange,
  onAsrEditingChange,
  onModelsBusyChange,
}: {
  active: boolean;
  visible: boolean;
  form: ReturnType<typeof useVoiceSecretarySettings>;
  leaveError: string;
  onProfileEditingChange: (editing: boolean) => void;
  onAsrEditingChange: (dirty: boolean, busy: boolean) => void;
  onModelsBusyChange: (busy: boolean) => void;
}) {
  const { t } = useTranslation("settings");
  const { t: tChat } = useTranslation("chat");
  const preferences = form.preferences;
  const secretaryIssue = secretaryReadinessIssue(form.state);
  const external = preferences.recognition_backend === "external_provider_asr";
  const local = preferences.recognition_backend === "assistant_service_local_asr";
  const effectiveRuntime =
    form.mode === "profile" ? form.selectedProfile?.runtime : form.settings.runtime;
  return (
    <section
      id="voice-settings-secretary-panel"
      role="tabpanel"
      hidden={!visible}
      aria-labelledby="voice-settings-secretary-tab"
      className="space-y-5"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <p className="min-w-0 max-w-2xl text-xs leading-5 text-[var(--color-text-muted)]">
          {t("voiceSettings.secretaryHint")}
        </p>
        <Button
          size="sm"
          variant="secondary"
          disabled={form.editingDisabled}
          onClick={() => void form.load()}
        >
          {t("voiceSettings.refresh")}
        </Button>
      </div>
      <StructuredRuntimeSettingsFields
        form={form}
        allowUnconfigured
        runtimeHint={t("voiceSettings.runtimeHint")}
        commandHint={t("voiceSettings.commandHint")}
      />
      <p className="text-xs leading-5 text-[var(--color-text-muted)]">
        {t("voiceSettings.nativePermissionHint")}
      </p>
      {effectiveRuntime === "claude" && (
        <p className="text-xs leading-5 text-[var(--color-text-secondary)]">
          {t("voiceSettings.claudeTrustHint")}
        </p>
      )}
      {secretaryIssue && secretaryIssue.code !== "not_configured" && (
        <p
          role="status"
          className="break-words text-xs leading-5 text-[var(--color-text-secondary)]"
        >
          <span>{t(secretaryIssue.titleKey)}</span>
          {secretaryIssue.detail && <span className="block">{secretaryIssue.detail}</span>}
        </p>
      )}
      <div className="space-y-3 border-t border-[var(--glass-border-subtle)] pt-4">
        {form.error && (
          <p role="alert" className="break-words text-sm text-rose-600 dark:text-rose-300">
            {form.error}
          </p>
        )}
        {form.notice && (
          <p role="status" className="text-sm text-emerald-700 dark:text-emerald-300">
            {form.notice}
          </p>
        )}
        <SettingsSaveBar
          label={t("common:save")}
          busy={form.saving}
          onSave={() => void form.save()}
          disabled={form.saveDisabled || !form.runtimeChanged}
          hint={t("voiceSettings.applyHint")}
          actions={
            <>
              <Button
                variant="secondary"
                disabled={form.editingDisabled || !(form.runtimeChanged || form.backlogAction)}
                onClick={form.discard}
              >
                {t("voiceSettings.discardRuntime")}
              </Button>
              <VoiceRuntimeProfileActions
                form={form}
                name="Voice Secretary"
                onEditingChange={onProfileEditingChange}
              />
            </>
          }
        />
      </div>
      <SettingsCard
        title={t("voiceSettings.captureTitle")}
        description={
          <>
            {t("voiceSettings.captureHint")} {t("voiceSettings.preferencesAutoSaveHint")}
          </>
        }
      >
        <SettingsRow label={t("assistants.recognitionBackend")}>
          <SelectCombobox
            ariaLabel={t("assistants.recognitionBackend")}
            value={preferences.recognition_backend}
            disabled={form.preferencesDisabled}
            className={inputClass()}
            items={(
              ["browser_asr", "assistant_service_local_asr", "external_provider_asr"] as const
            ).map((value) => ({ value, label: t(`assistants.backends.${value}`) }))}
            onChange={(value) =>
              void form.savePreferences({
                recognition_backend: value as typeof preferences.recognition_backend,
              })
            }
          />
        </SettingsRow>
        <SettingsRow label={t("voiceSettings.defaultLanguage")}>
          <SelectCombobox
            ariaLabel={t("voiceSettings.defaultLanguage")}
            value={preferences.recognition_language}
            disabled={form.preferencesDisabled}
            className={inputClass()}
            items={Array.from(
              new Set([
                preferences.recognition_language,
                "auto",
                "mixed",
                "zh-CN",
                "en-US",
                "ja-JP",
                "ko-KR",
                "fr-FR",
                "de-DE",
                "es-ES",
              ]),
            ).map((value) => ({
              value,
              label:
                value === "auto"
                  ? tChat("voiceSecretaryLanguageAuto")
                  : value === "mixed"
                    ? tChat("voiceSecretaryLanguageMixed")
                    : value,
            }))}
            onChange={(value) => void form.savePreferences({ recognition_language: value })}
          />
        </SettingsRow>
        <div className="px-5 py-3.5" hidden={!external}>
          <ExternalAsrSettings
            provider={preferences.external_asr_provider}
            onProviderChange={(value) =>
              void form.savePreferences({ external_asr_provider: value })
            }
            disabled={!active || !external || form.preferencesDisabled}
            onConfigured={() => undefined}
            onEditingStateChange={onAsrEditingChange}
          />
        </div>
        <div className="px-5 py-3.5" hidden={!local}>
          <LocalAsrModels
            isActive={active && local}
            busy={form.preferencesDisabled}
            configuredModelId={preferences.service_model_id || undefined}
            onBusyChange={onModelsBusyChange}
          />
        </div>
      </SettingsCard>
      <SettingsCard
        title={t("voiceSettings.processingTitle")}
        description={
          <>
            {t("voiceSettings.processingHint")} {t("voiceSettings.preferencesAutoSaveHint")}
          </>
        }
      >
        <SettingsRow
          label={t("assistants.documentAutoUpdateSwitch")}
          labelId="secretary-auto-document-label"
        >
          <SettingsSwitch
            labelledBy="secretary-auto-document-label"
            checked={preferences.auto_document_max_window_seconds !== null}
            disabled={form.preferencesDisabled}
            onChange={(checked) =>
              void form.savePreferences({ auto_document_max_window_seconds: checked ? 300 : null })
            }
          />
        </SettingsRow>
        {preferences.auto_document_max_window_seconds !== null && (
          <SettingsRow
            label={t("assistants.documentUpdateInterval")}
            htmlFor="secretary-document-interval"
            help={t("assistants.documentUpdateIntervalEnabledHint")}
          >
            <div className="flex items-center gap-3">
              <input
                id="secretary-document-interval"
                type="number"
                required
                aria-label={t("assistants.documentUpdateInterval")}
                min={10}
                max={300}
                step={1}
                className="h-9 w-28 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-3 text-sm tabular-nums text-[var(--color-text-primary)]"
                value={preferences.auto_document_max_window_seconds ?? 300}
                onBlur={(event) => {
                  if (event.currentTarget.reportValidity())
                    void form.savePreferences({
                      auto_document_max_window_seconds: Number(event.currentTarget.value),
                    });
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter") event.currentTarget.blur();
                }}
                disabled={form.preferencesDisabled}
                onChange={(event) =>
                  form.updatePreferences({
                    auto_document_max_window_seconds: Number(event.target.value),
                  })
                }
              />
              <span className="text-xs text-[var(--color-text-muted)]">
                {t("voiceSettings.seconds")}
              </span>
            </div>
          </SettingsRow>
        )}
        <details className="px-5 py-3.5">
          <summary className="cursor-pointer text-sm font-medium">
            {t("voiceSettings.workRules")}
          </summary>
          <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voiceSettings.workRulesHint")}
          </p>
          <textarea
            aria-label={t("voiceSettings.workRules")}
            className={`${inputClass()} mt-3 min-h-40 resize-y font-mono text-xs leading-5`}
            rows={7}
            value={preferences.guidance || form.state?.default_guidance || ""}
            disabled={form.preferencesDisabled}
            onChange={(event) => form.updatePreferences({ guidance: event.target.value })}
          />
          <div className="mt-3 flex flex-wrap justify-end gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={form.preferencesDisabled || !form.rulesChanged}
              onClick={form.discardRules}
            >
              {t("voiceSettings.discardRules")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={form.preferencesDisabled || !preferences.guidance}
              onClick={() => form.updatePreferences({ guidance: "" })}
            >
              {t("assistants.resetDocumentUpdateInterval")}
            </Button>
            <Button
              size="sm"
              disabled={form.preferencesDisabled || !form.rulesChanged}
              onClick={() => void form.savePreferences({ guidance: preferences.guidance })}
            >
              {t("voiceSettings.saveRules")}
            </Button>
          </div>
        </details>
        {(form.preferencesSaving || form.preferencesError || form.preferencesNotice) && (
          <div aria-live="polite" className="space-y-2 px-5 py-3.5 text-xs leading-5">
            <p
              role={form.preferencesError ? "alert" : "status"}
              className={
                form.preferencesError
                  ? "text-rose-600 dark:text-rose-300"
                  : "text-[var(--color-text-muted)]"
              }
            >
              {form.preferencesSaving
                ? t("voiceSettings.saving")
                : form.preferencesError || form.preferencesNotice}
            </p>
            {form.preferencesError && (
              <Button
                variant="secondary"
                size="sm"
                disabled={form.preferencesDisabled}
                onClick={() => void form.retryPreferences()}
              >
                {t("voiceSettings.retryPreferences")}
              </Button>
            )}
          </div>
        )}
      </SettingsCard>
      {!!form.state?.invalid_sources && (
        <p role="status" className="text-xs leading-5 text-[var(--color-text-secondary)]">
          {t("voiceSettings.sourcesInvalid", { count: form.state.invalid_sources })}
        </p>
      )}
      {form.configured && !!form.state?.backlog_sources && (
        <fieldset
          className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-4"
          disabled={form.preferencesDisabled}
        >
          <legend className="text-sm font-medium">
            {t("voiceSettings.backlogTitle", { count: form.state?.backlog_sources })}
          </legend>
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voiceSettings.backlogHint")}
          </p>
          {(["process", "hold"] as const).map((action) => (
            <label key={action} className="flex items-start gap-2 text-xs leading-5">
              <input
                type="radio"
                name="secretary-backlog"
                className="mt-1"
                checked={form.backlogAction === action}
                onChange={() => form.setBacklogAction(action)}
              />
              {t(
                action === "process" ? "voiceSettings.processBacklog" : "voiceSettings.holdBacklog",
              )}
            </label>
          ))}
          {!form.runtimeChanged ? (
            <Button size="sm" disabled={form.saveDisabled} onClick={() => void form.save()}>
              {t("voiceSettings.applyBacklog")}
            </Button>
          ) : (
            <p className="text-xs leading-5 text-[var(--color-text-muted)]">
              {t("voiceSettings.backlogWithRuntime")}
            </p>
          )}
        </fieldset>
      )}
      {leaveError && (
        <p role="alert" className="text-sm text-rose-600 dark:text-rose-300">
          {leaveError}
        </p>
      )}
    </section>
  );
}
