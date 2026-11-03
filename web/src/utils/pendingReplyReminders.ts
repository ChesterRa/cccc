export const PENDING_REPLY_REMINDERS_KEY = "cccc-show-pending-reply-reminders";
const storageListenerKey = Symbol.for("cccc.pendingReplyReminders.storageListener");
let unsavedPreference: boolean | undefined;

export function loadPendingReplyReminders(): boolean {
  if (unsavedPreference !== undefined) return unsavedPreference;
  try {
    return localStorage.getItem(PENDING_REPLY_REMINDERS_KEY) !== "false";
  } catch {
    return true;
  }
}

export function savePendingReplyReminders(show: boolean): void {
  try {
    localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, String(show));
    unsavedPreference = undefined;
  } catch {
    // A blocked or full store must not prevent changing the in-memory preference.
    unsavedPreference = show;
  }
}

/** Keep one subscription across store reloads, including development hot updates. */
export function subscribePendingReplyReminders(onChange: (show: boolean) => void): () => void {
  if (
    typeof window === "undefined" ||
    typeof window.addEventListener !== "function" ||
    typeof window.removeEventListener !== "function"
  )
    return () => {};
  const target = window as Window & { [storageListenerKey]?: () => void };
  target[storageListenerKey]?.();
  const listener = (event: StorageEvent) => {
    if (event.key !== PENDING_REPLY_REMINDERS_KEY && event.key !== null) return;
    if (unsavedPreference !== undefined) return;
    let show: boolean;
    try {
      const storage = target.localStorage;
      if (event.storageArea && event.storageArea !== storage) return;
      // Events can arrive after a newer write. Read the current value instead.
      show = storage.getItem(PENDING_REPLY_REMINDERS_KEY) !== "false";
    } catch {
      return;
    }
    onChange(show);
  };
  target.addEventListener("storage", listener);
  const unsubscribe = () => {
    target.removeEventListener("storage", listener);
    if (target[storageListenerKey] === unsubscribe) delete target[storageListenerKey];
  };
  target[storageListenerKey] = unsubscribe;
  return unsubscribe;
}

/** Leave every other filter and session field intact when reminders are hidden. */
export function clearPendingReplyFilters<Session>(
  sessions: Record<string, Session>,
): Record<string, Session> {
  let next = sessions;
  for (const [groupId, session] of Object.entries(sessions)) {
    if (
      !session ||
      typeof session !== "object" ||
      !("chatFilter" in session) ||
      session.chatFilter !== "pending_user"
    )
      continue;
    if (next === sessions) next = { ...sessions };
    next[groupId] = { ...session, chatFilter: "all" };
  }
  return next;
}

/** Patch the latest browser copy, never a tab's possibly stale session snapshot. */
export function clearStoredPendingReplyFilters(storageKey: string): void {
  try {
    const sessions = JSON.parse(localStorage.getItem(storageKey) || "null");
    if (!sessions || typeof sessions !== "object" || Array.isArray(sessions)) return;
    const next = clearPendingReplyFilters(sessions);
    if (next !== sessions) localStorage.setItem(storageKey, JSON.stringify(next));
  } catch {
    // Keep the in-memory fallback usable when storage is blocked or malformed.
  }
}
