import { useCallback, useEffect, useMemo, useState } from "react";
import { subscribeVoiceAudioStorage, useVoiceAudioStore } from "../../stores/useVoiceAudioStore";
import type { VoiceProvider } from "../../services/api";
import {
  loadCodexVoicePreferences,
  normalizeCodexRealtimeVoice,
  saveCodexVoicePreferences,
  type CodexVoicePreferences,
} from "./codexVoicePreferences";

export function useCodexVoicePreferencesState() {
  const [choice, setChoice] = useState(() => {
    const saved = loadCodexVoicePreferences();
    return {
      provider: saved.provider || ("codex" as VoiceProvider),
      codex: saved.voice,
      grok: saved.grokVoice || "eve",
    };
  });
  useEffect(
    () =>
      saveCodexVoicePreferences({
        voice: choice.codex,
        provider: choice.provider,
        grokVoice: choice.grok,
      }),
    [choice],
  );
  const audio = useVoiceAudioStore((state) => state.preferences);
  useEffect(() => subscribeVoiceAudioStorage(), []);
  const [catalogs, setCatalogs] = useState<Partial<Record<VoiceProvider, string[]>>>({});

  const updatePreferences = useCallback((next: Partial<CodexVoicePreferences>) => {
    if (next.inputDeviceId !== undefined || next.outputDeviceId !== undefined)
      useVoiceAudioStore.getState().update(next);
    setChoice((current) => {
      const provider = next.provider || current.provider;
      return {
        ...current,
        provider,
        ...(next.voice !== undefined
          ? {
              [provider]: normalizeCodexRealtimeVoice(
                next.voice,
                provider === "grok" ? "eve" : "cove",
              ),
            }
          : {}),
      };
    });
  }, []);

  const acceptSupportedVoices = useCallback(
    (voices: unknown, defaultVoice?: unknown, provider: VoiceProvider = "codex") => {
      if (!Array.isArray(voices)) return;
      const supported = [
        ...new Set(
          voices
            .filter((value): value is string => typeof value === "string" && !!value.trim())
            .map((voice) => normalizeCodexRealtimeVoice(voice)),
        ),
      ];
      if (supported.length === 0) return;
      const requestedDefault = normalizeCodexRealtimeVoice(defaultVoice);
      const fallback = supported.includes(requestedDefault) ? requestedDefault : supported[0];
      setCatalogs((current) => ({ ...current, [provider]: supported }));
      setChoice((current) =>
        supported.includes(current[provider]) ? current : { ...current, [provider]: fallback },
      );
    },
    [],
  );

  const preferences = useMemo<CodexVoicePreferences>(
    () => ({ provider: choice.provider, voice: choice[choice.provider], ...audio }),
    [choice, audio],
  );
  const supportedVoices = catalogs[choice.provider] || [];
  return { preferences, supportedVoices, updatePreferences, acceptSupportedVoices };
}
