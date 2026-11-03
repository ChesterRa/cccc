import { afterEach, beforeEach, vi } from "vite-plus/test";
import { PENDING_REPLY_REMINDERS_KEY } from "../utils/pendingReplyReminders";

export { PENDING_REPLY_REMINDERS_KEY };
export const sessionsKey = "cccc-chat-sessions";
const storageMethods = { getItem: localStorage.getItem, setItem: localStorage.setItem };
// happy-dom binds Storage methods on a Proxy whose delete trap cannot restore spies.
export const restoreStorageMethods = () => {
  for (const [name, value] of Object.entries(storageMethods))
    Object.defineProperty(localStorage, name, { value, configurable: true, writable: true });
};
export const storedSessions = () => JSON.parse(localStorage.getItem(sessionsKey) || "{}");
export const notifyStorage = (newValue: string | null, options: StorageEventInit = {}) =>
  window.dispatchEvent(
    new StorageEvent("storage", {
      key: PENDING_REPLY_REMINDERS_KEY,
      newValue,
      storageArea: localStorage,
      ...options,
    }),
  );

export function setupPendingReplyReminderStoreTest() {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
  });
  afterEach(() => {
    vi.restoreAllMocks();
    restoreStorageMethods();
  });
}
