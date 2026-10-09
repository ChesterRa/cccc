import { useCallback, useEffect, useMemo, useState } from "react";
import { subscribeVoiceAudioStorage, useVoiceAudioStore } from "../../stores/useVoiceAudioStore";
import {
  CODEX_REALTIME_VOICES,
  loadCodexVoicePreferences,
  normalizeCodexRealtimeVoice,
  saveCodexVoicePreferences,
  type CodexVoicePreferences,
} from "./codexVoicePreferences";

export function useCodexVoicePreferencesState() {
  const [voice, setVoice] = useState(() => loadCodexVoicePreferences().voice);
  const audio = useVoiceAudioStore((state) => state.preferences);
  useEffect(() => subscribeVoiceAudioStorage(), []);
  const [supportedVoices, setSupportedVoices] = useState<string[]>([...CODEX_REALTIME_VOICES]);

  const updatePreferences = useCallback((next: Partial<CodexVoicePreferences>) => {
    if (next.inputDeviceId !== undefined || next.outputDeviceId !== undefined)
      useVoiceAudioStore.getState().update(next);
    if (next.voice !== undefined) {
      const voice = normalizeCodexRealtimeVoice(next.voice);
      saveCodexVoicePreferences({ voice, ...useVoiceAudioStore.getState().preferences });
      setVoice(voice);
    }
  }, []);

  const acceptSupportedVoices = useCallback((voices: unknown) => {
    if (!Array.isArray(voices)) return;
    const supported = voices.filter((voice) => CODEX_REALTIME_VOICES.includes(voice as never));
    if (supported.length > 0) setSupportedVoices(supported);
  }, []);

  const preferences = useMemo(() => ({ voice, ...audio }), [voice, audio]);
  return { preferences, supportedVoices, updatePreferences, acceptSupportedVoices };
}
