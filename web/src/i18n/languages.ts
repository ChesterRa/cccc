export type LanguageCode = "en" | "zh" | "ja";

export const SUPPORTED_LANGUAGES: LanguageCode[] = ["en", "zh", "ja"];

export const LANGUAGE_NAME_KEY: Record<LanguageCode, "english" | "chinese" | "japanese"> = {
  en: "english",
  zh: "chinese",
  ja: "japanese",
};

export const LANGUAGE_SHORT_LABEL: Record<LanguageCode, string> = {
  en: "EN",
  zh: "中",
  ja: "日",
};

export function normalizeLanguageCode(language: string | undefined): LanguageCode {
  const normalized = String(language || "").toLowerCase();
  if (normalized.startsWith("zh")) return "zh";
  if (normalized.startsWith("ja")) return "ja";
  return "en";
}

