export const CODEX_REALTIME_VOICES = [
  "juniper",
  "maple",
  "spruce",
  "ember",
  "vale",
  "breeze",
  "arbor",
  "sol",
  "cove",
] as const;

export type CodexRealtimeVoice = (typeof CODEX_REALTIME_VOICES)[number];

export type CodexVoicePreferences = {
  voice: CodexRealtimeVoice;
  inputDeviceId: string;
  outputDeviceId: string;
};

export const DEFAULT_CODEX_REALTIME_VOICE: CodexRealtimeVoice = "cove";
const STORAGE_KEY = "cccc.codexVoice.preferences.v1";

export const DEFAULT_CODEX_VOICE_PREFERENCES: CodexVoicePreferences = {
  voice: DEFAULT_CODEX_REALTIME_VOICE,
  inputDeviceId: "",
  outputDeviceId: "",
};

export function normalizeCodexRealtimeVoice(value: unknown): CodexRealtimeVoice {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  return (
    CODEX_REALTIME_VOICES.find((candidate) => candidate === normalized) ||
    DEFAULT_CODEX_REALTIME_VOICE
  );
}

export function normalizeCodexVoicePreferences(value: unknown): CodexVoicePreferences {
  const record = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  return {
    voice: normalizeCodexRealtimeVoice(record.voice),
    inputDeviceId: boundedDeviceId(record.inputDeviceId),
    outputDeviceId: boundedDeviceId(record.outputDeviceId),
  };
}

export function loadCodexVoicePreferences(): CodexVoicePreferences {
  if (typeof window === "undefined") return { ...DEFAULT_CODEX_VOICE_PREFERENCES };
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    return raw
      ? normalizeCodexVoicePreferences(JSON.parse(raw))
      : { ...DEFAULT_CODEX_VOICE_PREFERENCES };
  } catch {
    return { ...DEFAULT_CODEX_VOICE_PREFERENCES };
  }
}

export function saveCodexVoicePreferences(preferences: CodexVoicePreferences): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(normalizeCodexVoicePreferences(preferences)),
    );
  } catch {
    // Browsers can disable local storage. The in-memory selection still works
    // for the current page and is safer than failing the call controls.
  }
}

export function formatCodexVoiceName(voice: string): string {
  const normalized = String(voice || "").trim();
  return normalized ? `${normalized[0].toUpperCase()}${normalized.slice(1)}` : "";
}

function boundedDeviceId(value: unknown): string {
  const normalized = typeof value === "string" ? value.trim() : "";
  return normalized.length <= 512 ? normalized : "";
}
