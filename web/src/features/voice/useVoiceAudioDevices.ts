import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

export function useVoiceAudioDevices(active: boolean) {
  const { t } = useTranslation("settings");
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [error, setError] = useState("");
  const sequence = useRef(0);
  const [loaded, setLoaded] = useState(false);
  const refresh = useCallback(async () => {
    const generation = ++sequence.current;
    if (!navigator.mediaDevices?.enumerateDevices) {
      setError(t("voiceAudio.unavailable"));
      return;
    }
    try {
      const next = await navigator.mediaDevices.enumerateDevices();
      if (generation !== sequence.current) return;
      setLoaded(true);
      setDevices(next.filter((device) => device.deviceId));
      setError("");
    } catch {
      if (generation === sequence.current) setError(t("voiceAudio.unavailable"));
    }
  }, [t]);
  const invalidate = useCallback(() => {
    sequence.current++;
  }, []);
  useEffect(() => {
    if (!active) return;
    void refresh();
    const media = navigator.mediaDevices;
    media?.addEventListener?.("devicechange", refresh);
    return () => {
      invalidate();
      media?.removeEventListener?.("devicechange", refresh);
    };
  }, [active, refresh, invalidate]);
  const inputs = useMemo(() => devices.filter((device) => device.kind === "audioinput"), [devices]);
  const outputs = useMemo(
    () => devices.filter((device) => device.kind === "audiooutput"),
    [devices],
  );
  const supportsOutputSelection =
    typeof HTMLMediaElement !== "undefined" &&
    typeof (HTMLMediaElement.prototype as HTMLMediaElement & { setSinkId?: unknown }).setSinkId ===
      "function";
  const label = (kind: "input" | "output", id: string) => {
    if (!id) return t("voiceAudio.systemDefault");
    if (!loaded && !error) return t("voiceAudio.loading");
    const list = kind === "input" ? inputs : outputs;
    const index = list.findIndex((device) => device.deviceId === id);
    return index < 0
      ? t("voiceAudio.savedUnavailable")
      : list[index].label ||
          t(kind === "input" ? "voiceAudio.microphoneNumber" : "voiceAudio.speakerNumber", {
            number: index + 1,
          });
  };
  return { inputs, outputs, refresh, error, loaded, supportsOutputSelection, label };
}
