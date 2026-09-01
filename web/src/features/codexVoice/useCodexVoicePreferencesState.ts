import { useCallback, useState } from "react";
import {
  CODEX_REALTIME_VOICES,
  loadCodexVoicePreferences,
  normalizeCodexRealtimeVoice,
  saveCodexVoicePreferences,
  type CodexVoicePreferences,
} from "./codexVoicePreferences";

export function useCodexVoicePreferencesState() {
  const [preferences, setPreferences] = useState<CodexVoicePreferences>(() =>
    loadCodexVoicePreferences(),
  );
  const [supportedVoices, setSupportedVoices] = useState<string[]>([...CODEX_REALTIME_VOICES]);

  const updatePreferences = useCallback((next: Partial<CodexVoicePreferences>) => {
    setPreferences((current) => {
      const updated: CodexVoicePreferences = {
        ...current,
        ...next,
        voice: normalizeCodexRealtimeVoice(next.voice ?? current.voice),
      };
      saveCodexVoicePreferences(updated);
      return updated;
    });
  }, []);

  const acceptSupportedVoices = useCallback((voices: unknown) => {
    if (!Array.isArray(voices)) return;
    const supported = voices.filter((voice) => CODEX_REALTIME_VOICES.includes(voice as never));
    if (supported.length > 0) setSupportedVoices(supported);
  }, []);

  return { preferences, supportedVoices, updatePreferences, acceptSupportedVoices };
}
