export const CHATGPT_APP_PERMISSION_HINT_DISMISSED_KEY = "cccc.chatgptAppPermissionHint.dismissed";
export const CHATGPT_APP_PERMISSION_HINT_DISMISSED_EVENT = "cccc:chatgpt-app-permission-hint-dismissed";

export function readChatGptAppPermissionHintDismissed(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(CHATGPT_APP_PERMISSION_HINT_DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function dismissChatGptAppPermissionHint(): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(CHATGPT_APP_PERMISSION_HINT_DISMISSED_KEY, "1");
  } catch {
    // Ignore storage failures; the in-memory UI state can still hide the notice.
  }
  window.dispatchEvent(new Event(CHATGPT_APP_PERMISSION_HINT_DISMISSED_EVENT));
}
