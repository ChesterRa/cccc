// @vitest-environment happy-dom
import { expect, it, vi } from "vite-plus/test";
import {
  PENDING_REPLY_REMINDERS_KEY,
  sessionsKey,
  storedSessions,
  notifyStorage,
  restoreStorageMethods,
  setupPendingReplyReminderStoreTest,
} from "./pendingReplyReminderStoreTestSupport";

setupPendingReplyReminderStoreTest();

it("loads preferences with a timer-only window and keeps state usable", async () => {
  vi.stubGlobal("window", { setTimeout, clearTimeout });
  try {
    const { useUIStore } = await import("./useUIStore");
    expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
    useUIStore.getState().setChatFilter("a", "pending_user");
    useUIStore.getState().setShowPendingReplyReminders(false);
    expect(useUIStore.getState().showPendingReplyReminders).toBe(false);
    expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("all");
    expect(localStorage.getItem(PENDING_REPLY_REMINDERS_KEY)).toBe("false");
  } finally {
    vi.unstubAllGlobals();
  }
});

it("syncs another tab, repairs pending sessions without echoing the preference, and guards stale callbacks", async () => {
  const { useUIStore } = await import("./useUIStore");
  const staleSetFilter = useUIStore.getState().setChatFilter;
  staleSetFilter("a", "pending_user");
  staleSetFilter("b", "pending_user");
  useUIStore.getState().setChatSenderFilter("a", ["worker"]);
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  const writes = vi.spyOn(localStorage, "setItem");

  notifyStorage("false");

  expect(useUIStore.getState().showPendingReplyReminders).toBe(false);
  expect(useUIStore.getState().chatSessions.a).toMatchObject({
    chatFilter: "all",
    chatSenderFilter: ["worker"],
  });
  expect(useUIStore.getState().chatSessions.b.chatFilter).toBe("all");
  expect(writes.mock.calls.map(([key]) => key)).toEqual([sessionsKey]);
  expect(storedSessions().a.chatFilter).toBe("all");
  staleSetFilter("a", "pending_user");
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("all");
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "true");
  writes.mockClear();
  notifyStorage("true");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("all");
  expect(writes).not.toHaveBeenCalled();
});

it("repairs a stale pending write before enabling and reloading, preserving newer fields", async () => {
  const { useUIStore } = await import("./useUIStore");
  useUIStore.getState().setChatFilter("a", "pending_user");
  useUIStore.getState().setShowPendingReplyReminders(false);
  // Another tab's old navigation ran before it received the preference event.
  const staleWrite = {
    ...storedSessions(),
    a: { ...storedSessions().a, chatFilter: "pending_user", extra: "newer" },
    b: { chatFilter: "user", chatSenderFilter: ["peer"], extra: { untouched: true } },
  };
  localStorage.setItem(sessionsKey, JSON.stringify(staleWrite));
  const writes = vi.spyOn(localStorage, "setItem");
  notifyStorage("false");
  expect(storedSessions()).toEqual({ ...staleWrite, a: { ...staleWrite.a, chatFilter: "all" } });
  expect(writes.mock.calls.map(([key]) => key)).toEqual([sessionsKey]);
  useUIStore.getState().setShowPendingReplyReminders(true);
  vi.resetModules();
  const reloaded = (await import("./useUIStore")).useUIStore.getState();
  expect(reloaded.showPendingReplyReminders).toBe(true);
  expect(reloaded.chatSessions.a.chatFilter).toBe("all");
  expect(reloaded.chatSessions.b).toMatchObject({ chatFilter: "user", chatSenderFilter: ["peer"] });
});

it("checks current storage before an old handler can write pending without receiving its event", async () => {
  const { useUIStore, getChatSession } = await import("./useUIStore");
  const staleSetFilter = useUIStore.getState().setChatFilter;
  staleSetFilter("other", "mail");
  const latest = { other: { chatFilter: "user", chatSenderFilter: ["peer"], extra: "keep" } };
  localStorage.setItem(sessionsKey, JSON.stringify(latest));
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);

  staleSetFilter("a", "pending_user");

  expect(useUIStore.getState().showPendingReplyReminders).toBe(false);
  expect(getChatSession("a", useUIStore.getState().chatSessions).chatFilter).toBe("all");
  expect(storedSessions()).toEqual(latest);
});

it("ignores unrelated and session-storage events but restores the default on clear", async () => {
  const { useUIStore } = await import("./useUIStore");
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  notifyStorage("false", { key: "unrelated" });
  notifyStorage("false", { storageArea: sessionStorage });
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
  useUIStore.getState().setShowPendingReplyReminders(false);
  notifyStorage(null, { key: null, storageArea: sessionStorage });
  expect(useUIStore.getState().showPendingReplyReminders).toBe(false);
  localStorage.clear();
  notifyStorage(null, { key: null });
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
});

it("replaces the old store's subscription when its module is reloaded", async () => {
  const previous = (await import("./useUIStore")).useUIStore;
  vi.resetModules();
  const current = (await import("./useUIStore")).useUIStore;
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  notifyStorage("false");
  expect(current.getState().showPendingReplyReminders).toBe(false);
  expect(previous.getState().showPendingReplyReminders).toBe(true);
});

it("ignores a delayed old false event after reminders have already been enabled again", async () => {
  const { useUIStore } = await import("./useUIStore");
  useUIStore.getState().setShowPendingReplyReminders(false);
  useUIStore.getState().setShowPendingReplyReminders(true);
  useUIStore.getState().setChatFilter("a", "pending_user");
  const writes = vi.spyOn(localStorage, "setItem");
  notifyStorage("false");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("pending_user");
  expect(writes).not.toHaveBeenCalled();
});

it("ignores a storage event when the authoritative preference cannot be read", async () => {
  const { useUIStore } = await import("./useUIStore");
  useUIStore.getState().setChatFilter("a", "pending_user");
  vi.spyOn(localStorage, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  notifyStorage("false");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("pending_user");
});

it("honors an unsaved local choice until a successful write resumes cross-tab synchronization", async () => {
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  const { useUIStore } = await import("./useUIStore");
  const { loadPendingReplyReminders } = await import("../utils/pendingReplyReminders");
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const write = vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("quota exceeded");
  });

  useUIStore.getState().setShowPendingReplyReminders(true);
  expect(localStorage.getItem(PENDING_REPLY_REMINDERS_KEY)).toBe("false");
  expect(loadPendingReplyReminders()).toBe(true);
  useUIStore.getState().setChatFilter("a", "pending_user");
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("pending_user");
  notifyStorage("false");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("pending_user");

  write.mockRestore();
  restoreStorageMethods();
  useUIStore.getState().setShowPendingReplyReminders(true);
  expect(localStorage.getItem(PENDING_REPLY_REMINDERS_KEY)).toBe("true");
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  notifyStorage("false");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(false);
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("all");
});

it("discards only the unsaved preference after a page reload", async () => {
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  const { useUIStore } = await import("./useUIStore");
  vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("quota exceeded");
  });
  useUIStore.getState().setShowPendingReplyReminders(true);
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
  vi.resetModules();
  const reloaded = (await import("./useUIStore")).useUIStore;
  expect(reloaded.getState().showPendingReplyReminders).toBe(false);
});

it("keeps in-memory controls usable when storage reads and writes throw", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(localStorage, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("full");
  });
  const { useUIStore } = await import("./useUIStore");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
  useUIStore.getState().setChatFilter("a", "pending_user");
  expect(() => useUIStore.getState().setShowPendingReplyReminders(false)).not.toThrow();
  expect(useUIStore.getState().showPendingReplyReminders).toBe(false);
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("all");
  useUIStore.getState().setChatFilter("a", "pending_user");
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("all");
});
