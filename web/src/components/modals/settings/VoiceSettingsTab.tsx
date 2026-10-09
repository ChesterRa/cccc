import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/button";
import { Switch } from "../../ui/switch";
import { SelectCombobox } from "../../SelectCombobox";
import { VoiceRuntimeProfileActions } from "../../../features/voice/VoiceRuntimeProfileActions";
import { VoiceAudioSettings } from "../../../features/voice/VoiceAudioSettings";
import { StructuredRuntimeSettingsFields } from "../StructuredRuntimeSettingsFields";
import { CodexVoiceSettingsPanel } from "../../../features/codexVoice/CodexVoiceSettingsPanel";
import type { CodexVoiceShellState } from "../../../features/codexVoice/useCodexVoiceShell";
import { LocalAsrModels } from "./LocalAsrModels";
import { ExternalAsrSettings } from "./ExternalAsrSettings";
import { useVoiceSecretarySettings } from "./useVoiceSecretarySettings";
import { inputClass, labelClass } from "./types";

export function VoiceSettingsTab({
  isActive,
  onBeforeLeaveChange,
  voice,
  section = "secretary",
  onSectionChange,
  audioRequest = 0,
  featureRequest = 0,
  initialFocusRef,
}: {
  voice: CodexVoiceShellState;
  section?: "secretary" | "realtime";
  onSectionChange: (section: "secretary" | "realtime") => void;
  isDark: boolean;
  audioRequest?: number;
  featureRequest?: number;
  initialFocusRef?: RefObject<HTMLElement | null>;
  isActive: boolean;
  onBeforeLeaveChange: (guard: () => boolean) => void;
}) {
  const { t } = useTranslation("settings");
  const { t: tChat } = useTranslation("chat");
  const form = useVoiceSecretarySettings(isActive);
  const featureTabs = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!isActive || !featureRequest || audioRequest) return;
    featureTabs.current?.scrollIntoView({ block: "start" });
    featureTabs.current
      ?.querySelector<HTMLButtonElement>('[aria-selected="true"]')
      ?.focus({ preventScroll: true });
  }, [isActive, featureRequest, audioRequest, section]);
  const [profileEditing, setProfileEditing] = useState(false);
  const [asrEditing, setAsrEditing] = useState({ dirty: false, busy: false });
  const [modelsBusy, setModelsBusy] = useState(false);
  const [leaveError, setLeaveError] = useState("");
  const updateAsrEditing = useCallback(
    (dirty: boolean, busy: boolean) => setAsrEditing({ dirty, busy }),
    [],
  );
  const beforeLeave = useCallback(() => {
    if (
      form.saving ||
      form.preferencesSaving ||
      form.profileSaving ||
      asrEditing.busy ||
      modelsBusy ||
      voice.analystSettings.saving ||
      voice.analystSettings.profileSaving ||
      profileEditing
    ) {
      setLeaveError(t("voiceSettings.waitForSave"));
      return false;
    }
    return (
      !(form.hasChanges || asrEditing.dirty) || window.confirm(t("voiceSettings.discardChanges"))
    );
  }, [
    form.saving,
    form.preferencesSaving,
    form.profileSaving,
    form.hasChanges,
    asrEditing,
    modelsBusy,
    voice.analystSettings.saving,
    voice.analystSettings.profileSaving,
    profileEditing,
    t,
  ]);
  useEffect(() => {
    onBeforeLeaveChange(beforeLeave);
    return () => onBeforeLeaveChange(() => true);
  }, [onBeforeLeaveChange, beforeLeave]);
  const preferences = form.preferences;
  const external = preferences.recognition_backend === "external_provider_asr";
  const local = preferences.recognition_backend === "assistant_service_local_asr";
  const effectiveRuntime =
    form.mode === "profile" ? form.selectedProfile?.runtime : form.settings.runtime;
  return (
    <div className="space-y-5" data-voice-settings>
      <header>
        <h3 className="text-base font-semibold">{t("voiceSettings.title")}</h3>
        <p className="mt-1 text-sm leading-6 text-[var(--color-text-muted)]">
          {t("voiceSettings.scopeHint")}
        </p>
      </header>
      <VoiceAudioSettings
        active={isActive}
        focusRequest={audioRequest}
        initialFocusRef={initialFocusRef}
      />
      <div
        ref={featureTabs}
        role="tablist"
        aria-label={t("voiceSettings.title")}
        className="flex border-b border-[var(--glass-border-subtle)]"
      >
        {(["secretary", "realtime"] as const).map((part, index, parts) => (
          <button
            key={part}
            type="button"
            role="tab"
            ref={(node) => {
              if (node && initialFocusRef && featureRequest && !audioRequest && section === part)
                initialFocusRef.current = node;
            }}
            id={`voice-settings-${part}-tab`}
            aria-controls={`voice-settings-${part}-panel`}
            aria-selected={section === part}
            tabIndex={section === part ? 0 : -1}
            className={`min-w-0 flex-1 border-b-2 px-2 py-3 text-sm font-medium sm:flex-none sm:px-5 ${section === part ? "border-[var(--color-accent-primary)]" : "border-transparent text-[var(--color-text-muted)]"}`}
            onClick={() => onSectionChange(part)}
            onKeyDown={(event) => {
              const next =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? 1
                    : ["ArrowRight", "ArrowLeft"].includes(event.key)
                      ? (index + 1) % 2
                      : undefined;
              if (next === undefined) return;
              event.preventDefault();
              onSectionChange(parts[next]);
              document.getElementById(`voice-settings-${parts[next]}-tab`)?.focus();
            }}
          >
            {t(`voiceSettings.${part}`)}
          </button>
        ))}
      </div>
      <section
        id="voice-settings-secretary-panel"
        role="tabpanel"
        hidden={section !== "secretary"}
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
        {(!!form.state?.settings.runtime || !!form.state?.settings.profile_id) &&
          !!form.state?.readiness_error &&
          !form.state.configured && (
            <p
              role="status"
              className="break-words text-xs leading-5 text-[var(--color-text-secondary)]"
            >
              {form.state.readiness_error}
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
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voiceSettings.applyHint")}
          </p>
          <div className="flex flex-wrap justify-end gap-2 [&>button]:min-w-0 [&>button]:whitespace-normal">
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
              onEditingChange={setProfileEditing}
            />
            <Button
              disabled={form.saveDisabled || !form.runtimeChanged}
              onClick={() => void form.save()}
            >
              {t(form.saving ? "voiceSettings.saving" : "common:save")}
            </Button>
          </div>
        </div>
        <section
          className="space-y-4 border-t border-[var(--glass-border-subtle)] pt-5"
          aria-labelledby="secretary-recognition-heading"
        >
          <div>
            <h4 id="secretary-recognition-heading" className="text-sm font-semibold">
              {t("voiceSettings.captureTitle")}
            </h4>
            <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
              {t("voiceSettings.captureHint")} {t("voiceSettings.preferencesAutoSaveHint")}
            </p>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <label className="block min-w-0">
              <span className={labelClass()}>{t("assistants.recognitionBackend")}</span>
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
            </label>
            <label className="block min-w-0">
              <span className={labelClass()}>{t("voiceSettings.defaultLanguage")}</span>
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
            </label>
          </div>
          <div hidden={!external}>
            <ExternalAsrSettings
              provider={preferences.external_asr_provider}
              onProviderChange={(value) =>
                void form.savePreferences({ external_asr_provider: value })
              }
              disabled={
                !isActive || section !== "secretary" || !external || form.preferencesDisabled
              }
              onConfigured={() => undefined}
              onEditingStateChange={updateAsrEditing}
            />
          </div>
          <div hidden={!local}>
            <LocalAsrModels
              isActive={isActive && section === "secretary" && local}
              busy={form.preferencesDisabled}
              configuredModelId={preferences.service_model_id || undefined}
              onBusyChange={setModelsBusy}
            />
          </div>
        </section>
        <section
          className="space-y-4 border-t border-[var(--glass-border-subtle)] pt-5"
          aria-labelledby="secretary-processing-heading"
        >
          <div>
            <h4 id="secretary-processing-heading" className="text-sm font-semibold">
              {t("voiceSettings.processingTitle")}
            </h4>
            <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
              {t("voiceSettings.processingHint")} {t("voiceSettings.preferencesAutoSaveHint")}
            </p>
          </div>
          <label className="flex items-center justify-between gap-3 text-sm">
            <span>{t("assistants.documentAutoUpdateSwitch")}</span>
            <Switch
              checked={preferences.auto_document_max_window_seconds !== null}
              disabled={form.preferencesDisabled}
              onChange={(event) =>
                void form.savePreferences({
                  auto_document_max_window_seconds: event.target.checked ? 300 : null,
                })
              }
            />
          </label>
          {preferences.auto_document_max_window_seconds !== null && (
            <label className="block min-w-0">
              <span className={labelClass()}>{t("assistants.documentUpdateInterval")}</span>
              <div className="flex items-center gap-3">
                <input
                  type="number"
                  required
                  aria-label={t("assistants.documentUpdateInterval")}
                  min={10}
                  max={300}
                  step={1}
                  className={inputClass() + " min-w-0 max-w-xs"}
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
              <span className="mt-1.5 block text-xs leading-5 text-[var(--color-text-muted)]">
                {t("assistants.documentUpdateIntervalEnabledHint")}
              </span>
            </label>
          )}
          <details className="border-t border-[var(--glass-border-subtle)] pt-4">
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
            <div aria-live="polite" className="space-y-2 text-xs leading-5">
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
        </section>
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
                  action === "process"
                    ? "voiceSettings.processBacklog"
                    : "voiceSettings.holdBacklog",
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
      <section
        id="voice-settings-realtime-panel"
        role="tabpanel"
        hidden={section !== "realtime"}
        aria-labelledby="voice-settings-realtime-tab"
      >
        <p className="mb-3 text-xs leading-5 text-[var(--color-text-muted)]">
          {t("voiceSettings.realtimeHint")}
        </p>
        <CodexVoiceSettingsPanel
          active={isActive && section === "realtime"}
          controller={voice.controller}
          analystSettings={voice.analystSettings}
          onAnalystSettingsActive={voice.setAnalystSettingsActive}
          onProfileEditingChange={setProfileEditing}
        />
      </section>
    </div>
  );
}
