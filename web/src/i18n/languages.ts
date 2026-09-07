export type LanguageCode = "en" | "zh" | "ja";

export const SUPPORTED_LANGUAGES: LanguageCode[] = ["en", "zh", "ja"];

export function normalizeLanguageCode(language: string | undefined): LanguageCode {
  const normalized = String(language || "").toLowerCase();
  if (normalized.startsWith("zh")) return "zh";
  if (normalized.startsWith("ja")) return "ja";
  return "en";
}
