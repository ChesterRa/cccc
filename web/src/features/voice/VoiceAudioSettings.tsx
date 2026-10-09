import { useEffect, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useVoiceAudioStore } from "../../stores/useVoiceAudioStore";
import { SelectCombobox } from "../../components/SelectCombobox";
import { Button } from "../../components/ui/button";
import { inputClass, labelClass } from "../../components/modals/settings/types";
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
      className="min-w-0 space-y-4 border-b border-[var(--glass-border-subtle)] pb-5"
      aria-labelledby="voice-audio-heading"
      data-voice-audio-settings
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 id="voice-audio-heading" className="text-sm font-semibold">
            {t("voiceAudio.title")}
          </h3>
          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voiceAudio.scope")}
          </p>
        </div>
        <Button variant="secondary" size="sm" onClick={() => void devices.refresh()}>
          {t("voiceSettings.refresh")}
        </Button>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <label className="block min-w-0">
          <span className={labelClass()}>{t("voiceAudio.microphone")}</span>
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
        </label>
        <div className="min-w-0">
          <label className="block min-w-0">
            <span className={labelClass()}>{t("voiceAudio.speaker")}</span>
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
          </label>
        </div>
      </div>
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
    </section>
  );
}
