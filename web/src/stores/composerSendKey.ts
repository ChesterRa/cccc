// Which keystroke sends the composer's message. This browser remembers the choice.
export type ComposerSendKey = "mod_enter" | "enter";

export const COMPOSER_SEND_KEY_STORAGE_KEY = "cccc-composer-send-key";
export const DEFAULT_COMPOSER_SEND_KEY: ComposerSendKey = "mod_enter";

export function normalizeComposerSendKey(value: unknown): ComposerSendKey {
  return value === "enter" ? "enter" : DEFAULT_COMPOSER_SEND_KEY;
}

export function loadComposerSendKeyPreference(): ComposerSendKey {
  try {
    if (typeof localStorage === "undefined") return DEFAULT_COMPOSER_SEND_KEY;
    return normalizeComposerSendKey(localStorage.getItem(COMPOSER_SEND_KEY_STORAGE_KEY));
  } catch (error) {
    console.warn("Failed to read composer send key from localStorage:", error);
    return DEFAULT_COMPOSER_SEND_KEY;
  }
}

export function saveComposerSendKeyPreference(sendKey: ComposerSendKey): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(COMPOSER_SEND_KEY_STORAGE_KEY, sendKey);
  } catch (error) {
    console.warn("Failed to persist composer send key to localStorage:", error);
  }
}

type EnterKeystroke = Pick<KeyboardEvent, "key" | "shiftKey" | "altKey" | "ctrlKey" | "metaKey">;

/**
 * Ctrl/Cmd+Enter always sends. With the Enter preference a bare Enter sends too,
 * and Shift+Enter (or Alt+Enter) still starts a new line. Input-method composition
 * is the caller's to exclude before asking.
 */
export function isComposerSendKeystroke(sendKey: ComposerSendKey, event: EnterKeystroke): boolean {
  if (event.key !== "Enter") return false;
  if (event.ctrlKey || event.metaKey) return true;
  return sendKey === "enter" && !event.shiftKey && !event.altKey;
}
