import { create } from "zustand";

export type VoiceAudioPreferences = { inputDeviceId: string; outputDeviceId: string };
export const VOICE_AUDIO_STORAGE_KEY = "cccc.voice.audio.v1";
const defaults: VoiceAudioPreferences = { inputDeviceId: "", outputDeviceId: "" };

function deviceId(value: unknown): string {
  const id = typeof value === "string" ? value.trim() : "";
  return id.length <= 512 ? id : "";
}
function read(key: string): VoiceAudioPreferences | null {
  try {
    const raw = window.localStorage.getItem(key);
    if (!raw) return null;
    const value = JSON.parse(raw);
    if (!value || typeof value !== "object") return null;
    return {
      inputDeviceId: deviceId(value.inputDeviceId),
      outputDeviceId: deviceId(value.outputDeviceId),
    };
  } catch {
    return null;
  }
}
let pendingStorageWrite = false;
function persist(preferences: VoiceAudioPreferences) {
  try {
    window.localStorage.setItem(VOICE_AUDIO_STORAGE_KEY, JSON.stringify(preferences));
    pendingStorageWrite = false;
  } catch {
    pendingStorageWrite = true; /* Keep the in-memory choice when browser storage is unavailable. */
  }
}
function initial(): VoiceAudioPreferences {
  const current = read(VOICE_AUDIO_STORAGE_KEY);
  if (current) return current;
  // Preserve an existing device choice once when moving it out of Codex Voice.
  const previous = read("cccc.codexVoice.preferences.v1");
  if (previous) {
    persist(previous);
    return previous;
  }
  return { ...defaults };
}

export const useVoiceAudioStore = create<{
  preferences: VoiceAudioPreferences;
  storageError: boolean;
  update: (patch: Partial<VoiceAudioPreferences>) => void;
}>((set) => ({
  preferences: initial(),
  storageError: pendingStorageWrite,
  update: (patch) =>
    set((state) => {
      const preferences = {
        inputDeviceId:
          patch.inputDeviceId === undefined
            ? state.preferences.inputDeviceId
            : deviceId(patch.inputDeviceId),
        outputDeviceId:
          patch.outputDeviceId === undefined
            ? state.preferences.outputDeviceId
            : deviceId(patch.outputDeviceId),
      };
      persist(preferences);
      return { preferences, storageError: pendingStorageWrite };
    }),
}));

export function refreshVoiceAudioPreferences() {
  if (pendingStorageWrite) return;
  const preferences = read(VOICE_AUDIO_STORAGE_KEY);
  const current = useVoiceAudioStore.getState().preferences;
  if (
    preferences &&
    (preferences.inputDeviceId !== current.inputDeviceId ||
      preferences.outputDeviceId !== current.outputDeviceId)
  )
    useVoiceAudioStore.setState({ preferences });
}
export function voiceAudioSnapshot(): VoiceAudioPreferences {
  refreshVoiceAudioPreferences();
  return { ...useVoiceAudioStore.getState().preferences };
}
export function subscribeVoiceAudioStorage(): () => void {
  const changed = (event: StorageEvent) => {
    try {
      if (event.storageArea !== window.localStorage) return;
    } catch {
      return;
    }
    if (event.key === null || (event.key === VOICE_AUDIO_STORAGE_KEY && !event.newValue))
      useVoiceAudioStore.setState({ preferences: { ...defaults } });
    else if (event.key === VOICE_AUDIO_STORAGE_KEY) refreshVoiceAudioPreferences();
  };
  window.addEventListener("storage", changed);
  window.addEventListener("focus", refreshVoiceAudioPreferences);
  return () => {
    window.removeEventListener("storage", changed);
    window.removeEventListener("focus", refreshVoiceAudioPreferences);
  };
}
