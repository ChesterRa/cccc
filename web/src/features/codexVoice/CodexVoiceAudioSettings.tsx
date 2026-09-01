import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { HeadphonesIcon, MicrophoneIcon, VolumeIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { formatCodexVoiceName } from "./codexVoicePreferences";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function CodexVoiceAudioSettings({
  active,
  controller,
}: {
  active: boolean;
  controller: CodexVoiceSessionController;
}) {
  const { t } = useTranslation("modals");
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [deviceError, setDeviceError] = useState("");
  const supportsOutputSelection =
    typeof HTMLMediaElement !== "undefined" &&
    typeof (HTMLMediaElement.prototype as HTMLMediaElement & { setSinkId?: unknown }).setSinkId ===
      "function";

  const refreshDevices = useCallback(async () => {
    if (!navigator.mediaDevices?.enumerateDevices) {
      setDevices([]);
      setDeviceError(t("codexVoiceDevicesUnavailable"));
      return;
    }
    try {
      setDevices(await navigator.mediaDevices.enumerateDevices());
      setDeviceError("");
    } catch {
      setDevices([]);
      setDeviceError(t("codexVoiceDevicesUnavailable"));
    }
  }, [t]);

  useEffect(() => {
    if (!active) return;
    void refreshDevices();
    const media = navigator.mediaDevices;
    if (!media?.addEventListener) return;
    const onChange = () => void refreshDevices();
    media.addEventListener("devicechange", onChange);
    return () => media.removeEventListener("devicechange", onChange);
  }, [active, refreshDevices]);

  const inputs = useMemo(() => devices.filter((device) => device.kind === "audioinput"), [devices]);
  const outputs = useMemo(
    () => devices.filter((device) => device.kind === "audiooutput"),
    [devices],
  );
  const selectClass =
    "mt-1.5 h-10 w-full rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 text-sm text-[var(--color-text-primary)] outline-none transition-colors focus-visible:border-[var(--color-accent-primary)] focus-visible:ring-2 focus-visible:ring-[var(--color-accent-primary)]/20";

  return (
    <div className="grid gap-4 border-b border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-5 py-4 sm:grid-cols-3 sm:px-6">
      <label className="block text-xs font-medium text-[var(--color-text-secondary)]">
        <span className="inline-flex items-center gap-1.5">
          <HeadphonesIcon size={14} aria-hidden="true" />
          {t("codexVoiceVoiceLabel")}
        </span>
        <select
          className={selectClass}
          value={controller.preferences.voice}
          onChange={(event) => controller.updatePreferences({ voice: event.target.value as never })}
        >
          {controller.supportedVoices.map((voice) => (
            <option key={voice} value={voice}>
              {formatCodexVoiceName(voice)}
            </option>
          ))}
        </select>
        <span className="mt-1.5 block text-[11px] leading-4 text-[var(--color-text-muted)]">
          {controller.isEngaged ? t("codexVoiceVoiceNextCall") : t("codexVoiceVoiceHint")}
        </span>
      </label>

      <label className="block text-xs font-medium text-[var(--color-text-secondary)]">
        <span className="inline-flex items-center gap-1.5">
          <MicrophoneIcon size={14} aria-hidden="true" />
          {t("codexVoiceMicrophoneLabel")}
        </span>
        <select
          className={selectClass}
          value={controller.preferences.inputDeviceId}
          onChange={(event) => controller.updatePreferences({ inputDeviceId: event.target.value })}
        >
          <option value="">{t("codexVoiceSystemDefault")}</option>
          {inputs.map((device, index) => (
            <option key={device.deviceId} value={device.deviceId}>
              {device.label || t("codexVoiceMicrophoneFallback", { number: index + 1 })}
            </option>
          ))}
        </select>
      </label>

      <div>
        {supportsOutputSelection ? (
          <label className="block text-xs font-medium text-[var(--color-text-secondary)]">
            <span className="inline-flex items-center gap-1.5">
              <VolumeIcon size={14} aria-hidden="true" />
              {t("codexVoiceSpeakerLabel")}
            </span>
            <select
              className={selectClass}
              value={controller.preferences.outputDeviceId}
              onChange={(event) =>
                controller.updatePreferences({ outputDeviceId: event.target.value })
              }
            >
              <option value="">{t("codexVoiceSystemDefault")}</option>
              {outputs.map((device, index) => (
                <option key={device.deviceId} value={device.deviceId}>
                  {device.label || t("codexVoiceSpeakerFallback", { number: index + 1 })}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <div className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("codexVoiceOutputSelectionUnavailable")}
          </div>
        )}
        <div className="mt-1.5 flex items-start justify-between gap-2 text-[11px] leading-4 text-[var(--color-text-muted)]">
          <span>{deviceError || t("codexVoiceDevicePrivacyHint")}</span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-6 flex-none px-1.5 text-[11px]"
            onClick={() => void refreshDevices()}
          >
            {t("codexVoiceRefreshDevices")}
          </Button>
        </div>
      </div>
    </div>
  );
}
