import type { ActorSecretChanges } from "./actorSecretManagerModel";

export const DEEPSEEK_BASE_URL = "https://api.deepseek.com";
export const DEEPSEEK_MODEL = "deepseek-flash";
export const DEEPSEEK_SETTING_KEYS = [
  "DEEPSEEK_BASE_URL",
  "DEEPSEEK_MODEL",
  "DEEPSEEK_API_KEY",
] as const;

export function normalizeDeepSeekSettings(changes: ActorSecretChanges): ActorSecretChanges {
  const setVars = { ...changes.setVars };
  if (setVars.DEEPSEEK_BASE_URL !== undefined) {
    setVars.DEEPSEEK_BASE_URL = setVars.DEEPSEEK_BASE_URL.trim()
      .replace(/\/+$/, "")
      .replace(/\/chat\/completions$/, "");
  }
  if (setVars.DEEPSEEK_MODEL !== undefined) setVars.DEEPSEEK_MODEL = setVars.DEEPSEEK_MODEL.trim();
  // A pasted key often carries a trailing newline, which the provider rejects.
  if (setVars.DEEPSEEK_API_KEY !== undefined)
    setVars.DEEPSEEK_API_KEY = setVars.DEEPSEEK_API_KEY.trim();
  // A field left blank keeps the saved value, so one that trims to nothing is not a new value.
  for (const key of DEEPSEEK_SETTING_KEYS) {
    if (setVars[key] !== undefined && !setVars[key]) delete setVars[key];
  }
  return { ...changes, setVars };
}

export function deepSeekSettingsError(values: Record<string, string>): string {
  const base = values.DEEPSEEK_BASE_URL?.trim();
  if (base) {
    try {
      const url = new URL(base);
      if (
        !["http:", "https:"].includes(url.protocol) ||
        url.username ||
        url.password ||
        url.search ||
        url.hash
      )
        return "hostInvalid";
    } catch {
      return "hostInvalid";
    }
  }
  const model = values.DEEPSEEK_MODEL?.trim();
  if (model && (/\s/.test(model) || model.length > 256)) return "modelInvalid";
  return "";
}
