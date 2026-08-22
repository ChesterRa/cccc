export type ComposerMessageMode = "send" | "request_reply" | "mail";
export type ReplyMessageMode = "send" | "mail";

export const COMPOSER_MESSAGE_MODE_STORAGE_KEY = "cccc-composer-message-mode";
export const DEFAULT_COMPOSER_MESSAGE_MODE: ComposerMessageMode = "send";

export function normalizeComposerMessageMode(value: unknown): ComposerMessageMode {
  return value === "send" || value === "request_reply" || value === "mail"
    ? value
    : DEFAULT_COMPOSER_MESSAGE_MODE;
}

export function normalizeReplyMessageMode(value: unknown): ReplyMessageMode {
  return value === "mail" ? "mail" : "send";
}

export function loadComposerMessageModePreference(): ComposerMessageMode {
  try {
    if (typeof localStorage === "undefined") return DEFAULT_COMPOSER_MESSAGE_MODE;
    return normalizeComposerMessageMode(localStorage.getItem(COMPOSER_MESSAGE_MODE_STORAGE_KEY));
  } catch (error) {
    console.warn("Failed to read composer message mode from localStorage:", error);
    return DEFAULT_COMPOSER_MESSAGE_MODE;
  }
}

export function saveComposerMessageModePreference(mode: ComposerMessageMode): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(COMPOSER_MESSAGE_MODE_STORAGE_KEY, mode);
  } catch (error) {
    console.warn("Failed to persist composer message mode to localStorage:", error);
  }
}
