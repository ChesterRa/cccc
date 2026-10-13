import { useEffect, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useVoiceAudioStore } from "../../stores/useVoiceAudioStore";
import { SelectCombobox } from "../../components/SelectCombobox";
import { Button } from "../../components/ui/button";
import { inputClass } from "../../components/modals/settings/types";
import { SettingsCard, SettingsRow } from "../../components/modals/settings/SettingsKit";
import { useVoiceAudioDevices } from "./useVoiceAudioDevices";

export function VoiceAudioSettings({
  active,
  focusRequest = 0,
  initialFocusRef,
}: {
  active: boolean;
  focusRequest?: number;
  initialFocusRef?: RefObject<HTMLElement | null>;
}) {
  const { t } = useTranslation("settings");
  const { preferences, update, storageError } = useVoiceAudioStore();
  const devices = useVoiceAudioDevices(active);
  const error = storageError ? t("voiceAudio.storageUnavailable") : devices.error;
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!active || !focusRequest) return;
    ref.current?.scrollIntoView({ block: "start" });
    ref.current
      ?.querySelector<HTMLButtonElement>('[role="combobox"]')
      ?.focus({ preventScroll: true });
  }, [active, focusRequest]);
  return (
    <section
      ref={(node) => {
        ref.current = node;
        if (node && focusRequest && initialFocusRef)
          initialFocusRef.current = node.querySelector<HTMLElement>('[role="combobox"]');
      }}
      className="min-w-0"
      aria-labelledby="voice-audio-heading"
      data-voice-audio-settings
    >
      <SettingsCard
        title={<span id="voice-audio-heading">{t("voiceAudio.title")}</span>}
        description={t("voiceAudio.scope")}
        actions={
          <Button variant="secondary" size="sm" onClick={() => void devices.refresh()}>
            {t("voiceSettings.refresh")}
          </Button>
        }
      >
        <div className="grid divide-y divide-[var(--glass-border-subtle)] @min-[600px]/settings:grid-cols-2 @min-[600px]/settings:divide-y-0 @min-[600px]/settings:divide-x">
          <SettingsRow label={t("voiceAudio.microphone")} stacked>
            <SelectCombobox
              ariaLabel={t("voiceAudio.microphone")}
              className={inputClass()}
              value={preferences.inputDeviceId}
              placeholder={t(
                devices.loaded || devices.error
                  ? "voiceAudio.savedUnavailable"
                  : "voiceAudio.loading",
              )}
              onChange={(inputDeviceId) => update({ inputDeviceId })}
              items={[
                { value: "", label: t("voiceAudio.systemDefault") },
                ...devices.inputs.map((device) => ({
                  value: device.deviceId,
                  label: devices.label("input", device.deviceId),
                })),
              ]}
            />
          </SettingsRow>
          <SettingsRow label={t("voiceAudio.speaker")} stacked>
            {devices.supportsOutputSelection ? (
              <SelectCombobox
                ariaLabel={t("voiceAudio.speaker")}
                className={inputClass()}
                value={preferences.outputDeviceId}
                placeholder={t(
                  devices.loaded || devices.error
                    ? "voiceAudio.savedUnavailable"
                    : "voiceAudio.loading",
                )}
                onChange={(outputDeviceId) => update({ outputDeviceId })}
                items={[
                  { value: "", label: t("voiceAudio.systemDefault") },
                  ...devices.outputs.map((device) => ({
                    value: device.deviceId,
                    label: devices.label("output", device.deviceId),
                  })),
                ]}
              />
            ) : (
              <p className="text-xs leading-5 text-[var(--color-text-muted)]">
                {t("voiceAudio.systemSpeaker")}
              </p>
            )}
          </SettingsRow>
        </div>
        <div className="space-y-2 px-5 py-3.5">
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voiceAudio.applyHint")} {t("voiceAudio.browserAsrHint")}
          </p>
          <p
            role={error ? "alert" : undefined}
            className={
              error
                ? "text-xs leading-5 text-rose-600 dark:text-rose-400"
                : "text-xs leading-5 text-[var(--color-text-muted)]"
            }
          >
            {error || t("voiceAudio.permissionHint")}
          </p>
        </div>
      </SettingsCard>
    </section>
  );
}
