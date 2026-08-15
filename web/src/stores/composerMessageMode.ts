export type ComposerMessageMode = "normal" | "attention" | "reply";

export const COMPOSER_MESSAGE_MODE_STORAGE_KEY = "cccc-composer-message-mode";
export const DEFAULT_COMPOSER_MESSAGE_MODE: ComposerMessageMode = "reply";

export function normalizeComposerMessageMode(value: unknown): ComposerMessageMode {
  return value === "normal" || value === "attention" || value === "reply"
    ? value
    : DEFAULT_COMPOSER_MESSAGE_MODE;
}

export function getComposerMessageMode(
  priority: "normal" | "attention",
  replyRequired: boolean,
): ComposerMessageMode {
  if (replyRequired) return "reply";
  return priority === "attention" ? "attention" : "normal";
}

export function deliveryStateForMessageMode(mode: ComposerMessageMode): {
  priority: "normal" | "attention";
  replyRequired: boolean;
} {
  if (mode === "attention") return { priority: "attention", replyRequired: false };
  if (mode === "reply") return { priority: "normal", replyRequired: true };
  return { priority: "normal", replyRequired: false };
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
