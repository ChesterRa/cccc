import type { VoiceProvider } from "../../services/api";

export type CodexVoicePreferences = {
  provider?: VoiceProvider;
  grokVoice?: string;
  voice: string;
  inputDeviceId: string;
  outputDeviceId: string;
};

export const DEFAULT_CODEX_REALTIME_VOICE = "cove";
const STORAGE_KEY = "cccc.codexVoice.preferences.v1";

// Documented ChatGPT Live voice profiles, checked 2026-10-16:
// https://help.openai.com/en/articles/20001274-chatgpt-voice
// Display hints only; the server still owns the supported-voice catalog.
// Porto and Selva have no documented language in this source, so remain unlabeled.
const CODEX_VOICE_PRIMARY_LANGUAGES: Readonly<Record<string, string>> = {
  aube: "fr",
  bubbie: "en",
  haetsal: "ko",
  hanul: "ko",
  himari: "ja",
  leher: "hi",
  miko: "de",
  neer: "hi",
  rindo: "ja",
  rio: "pt-BR",
  rivage: "fr",
  sonna: "de",
  tinta: "es-MX",
  viola: "pt-BR",
};

export function codexVoicePrimaryLanguage(voice: string): string | undefined {
  return CODEX_VOICE_PRIMARY_LANGUAGES[voice.trim().toLowerCase()];
}

export const DEFAULT_CODEX_VOICE_PREFERENCES: CodexVoicePreferences = {
  voice: DEFAULT_CODEX_REALTIME_VOICE,
  inputDeviceId: "",
  outputDeviceId: "",
};

export function normalizeCodexRealtimeVoice(
  value: unknown,
  fallback = DEFAULT_CODEX_REALTIME_VOICE,
): string {
  return (typeof value === "string" && value.trim().toLowerCase()) || fallback;
}

export function normalizeCodexVoicePreferences(value: unknown): CodexVoicePreferences {
  const record = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  return {
    ...(record.provider === "codex" || record.provider === "grok"
      ? { provider: record.provider }
      : {}),
    ...(typeof record.grokVoice === "string"
      ? { grokVoice: normalizeCodexRealtimeVoice(record.grokVoice, "eve") }
      : {}),
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

export function saveCodexVoicePreferences(
  preferences: Pick<CodexVoicePreferences, "voice" | "provider" | "grokVoice">,
): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        voice: normalizeCodexRealtimeVoice(preferences.voice),
        ...(preferences.provider ? { provider: preferences.provider } : {}),
        ...(preferences.grokVoice
          ? { grokVoice: normalizeCodexRealtimeVoice(preferences.grokVoice, "eve") }
          : {}),
      }),
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
