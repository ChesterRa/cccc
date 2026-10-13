import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { SettingsPage } from "./SettingsKit";
import { VoiceAudioSettings } from "../../../features/voice/VoiceAudioSettings";
import { CodexVoiceSettingsPanel } from "../../../features/codexVoice/CodexVoiceSettingsPanel";
import type { CodexVoiceShellState } from "../../../features/codexVoice/useCodexVoiceShell";
import { VoiceSecretarySettings } from "./VoiceSecretarySettings";
import { useVoiceSecretarySettings } from "./useVoiceSecretarySettings";

export function VoiceSettingsTab({
  isActive,
  onBeforeLeaveChange,
  voice,
  section = "secretary",
  onSectionChange,
  audioRequest = 0,
  featureRequest = 0,
  analystRequest = 0,
  initialFocusRef,
}: {
  voice: CodexVoiceShellState;
  section?: "secretary" | "realtime";
  onSectionChange: (section: "secretary" | "realtime") => void;
  isDark: boolean;
  audioRequest?: number;
  featureRequest?: number;
  analystRequest?: number;
  initialFocusRef?: RefObject<HTMLElement | null>;
  isActive: boolean;
  onBeforeLeaveChange: (guard: () => boolean) => void;
}) {
  const { t } = useTranslation("settings");
  const form = useVoiceSecretarySettings(isActive);
  const featureTabs = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!isActive || !featureRequest || audioRequest || analystRequest) return;
    featureTabs.current?.scrollIntoView({ block: "start" });
    featureTabs.current
      ?.querySelector<HTMLButtonElement>('[aria-selected="true"]')
      ?.focus({ preventScroll: true });
  }, [isActive, featureRequest, audioRequest, analystRequest, section]);
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
  return (
    <div data-voice-settings>
      <SettingsPage title={t("voiceSettings.title")} description={t("voiceSettings.scopeHint")}>
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
        <VoiceSecretarySettings
          active={isActive && section === "secretary"}
          visible={section === "secretary"}
          form={form}
          leaveError={leaveError}
          onProfileEditingChange={setProfileEditing}
          onAsrEditingChange={updateAsrEditing}
          onModelsBusyChange={setModelsBusy}
        />
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
            analystRequest={analystRequest}
            initialFocusRef={initialFocusRef}
            controller={voice.controller}
            analystSettings={voice.analystSettings}
            onAnalystSettingsActive={voice.setAnalystSettingsActive}
            onProfileEditingChange={setProfileEditing}
          />
        </section>
      </SettingsPage>
    </div>
  );
}
