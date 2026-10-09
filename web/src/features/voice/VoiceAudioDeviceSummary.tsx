import { useTranslation } from "react-i18next";
import { useVoiceAudioStore, type VoiceAudioPreferences } from "../../stores/useVoiceAudioStore";
import { useVoiceAudioDevices } from "./useVoiceAudioDevices";
import { Button } from "../../components/ui/button";

export function VoiceAudioDeviceSummary({
  active,
  snapshot,
  browserAsr = false,
  withSpeaker = false,
  onAdjust,
}: {
  active: boolean;
  snapshot?: VoiceAudioPreferences | null;
  browserAsr?: boolean;
  withSpeaker?: boolean;
  onAdjust: () => void;
}) {
  const { t } = useTranslation("settings");
  const preferences = useVoiceAudioStore((state) => state.preferences);
  const devices = useVoiceAudioDevices(active);
  const current = snapshot || preferences;
  return (
    <div
      className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs leading-5 text-[var(--color-text-muted)]"
      data-voice-device-summary
    >
      <span>{t(snapshot ? "voiceAudio.currentDevices" : "voiceAudio.nextDevices")}</span>
      <span className="min-w-0 break-words">
        {t("voiceAudio.microphone")}:{" "}
        {browserAsr
          ? t("voiceAudio.browserDefault")
          : devices.label("input", current.inputDeviceId)}
      </span>
      {withSpeaker && (
        <span className="min-w-0 break-words">
          {t("voiceAudio.speaker")}:{" "}
          {devices.supportsOutputSelection
            ? devices.label("output", current.outputDeviceId)
            : t("voiceAudio.systemDefault")}
        </span>
      )}
      <Button variant="ghost" size="sm" className="min-w-0 whitespace-normal" onClick={onAdjust}>
        {t("voiceAudio.adjust")}
      </Button>
    </div>
  );
}
