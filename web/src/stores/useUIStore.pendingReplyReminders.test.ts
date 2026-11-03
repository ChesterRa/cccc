// @vitest-environment happy-dom
import { expect, it, vi } from "vite-plus/test";
import {
  PENDING_REPLY_REMINDERS_KEY,
  sessionsKey,
  storedSessions,
  setupPendingReplyReminderStoreTest,
} from "./pendingReplyReminderStoreTestSupport";

setupPendingReplyReminderStoreTest();

it.each([
  [null, true],
  ["false", false],
  ["true", true],
  ["invalid", true],
] as const)("restores the browser preference %s as %s", async (stored, expected) => {
  if (stored !== null) localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, stored);
  const { useUIStore } = await import("./useUIStore");
  expect(useUIStore.getState().showPendingReplyReminders).toBe(expected);
});

it("resets pending filters for every Group while preserving senders and other session state", async () => {
  const { useUIStore } = await import("./useUIStore");
  const actions = useUIStore.getState();
  actions.setChatFilter("a", "pending_user");
  actions.setChatSenderFilter("a", ["worker"]);
  actions.setGroupWorkView("a", "terminals");
  actions.setGroupTerminalPage("a", 3);
  actions.setChatMobileSurface("a", "files");
  actions.setChatSidePanelLayout("a", { width: 480, compact: false });
  actions.setChatVoicePreferences("a", {
    voiceCaptureMode: "document",
    voicePromptAutoRefine: true,
  });
  actions.setChatUnreadCount("a", 7);
  actions.setShowScrollButton("a", true);
  actions.setChatScrollSnapshot("a", {
    coordinateVersion: 1,
    mode: "detached",
    anchorId: "event-a",
    offsetPx: 12,
    updatedAt: 123,
  });
  actions.setChatFilter("b", "pending_user");
  actions.setChatSenderFilter("b", ["leader"]);
  for (const filter of ["mail", "request_reply", "user", "all"] as const) {
    actions.setChatSenderFilter(filter, ["peer"]);
    actions.setChatFilter(filter, filter);
  }
  const before = useUIStore.getState().chatSessions;

  actions.setShowPendingReplyReminders(false);

  const next = useUIStore.getState();
  expect(next.showPendingReplyReminders).toBe(false);
  expect(localStorage.getItem(PENDING_REPLY_REMINDERS_KEY)).toBe("false");
  expect(next.chatSessions.a).toEqual({ ...before.a, chatFilter: "all" });
  expect(next.chatSessions.b).toEqual({ ...before.b, chatFilter: "all" });
  for (const filter of ["mail", "request_reply", "user", "all"])
    expect(next.chatSessions[filter]).toEqual(before[filter]);
  expect(storedSessions().a).toMatchObject({
    chatFilter: "all",
    chatSenderFilter: ["worker"],
    terminalPage: 3,
  });
  expect(storedSessions().b).toMatchObject({ chatFilter: "all", chatSenderFilter: ["leader"] });

  vi.resetModules();
  const reloaded = (await import("./useUIStore")).useUIStore.getState();
  expect(reloaded.showPendingReplyReminders).toBe(false);
  expect(reloaded.chatSessions.a).toMatchObject({
    chatFilter: "all",
    chatSenderFilter: ["worker"],
    workView: "terminals",
    terminalPage: 3,
    mobileSurface: "files",
    sidePanelWidth: 480,
    presentationCompact: false,
    voiceCaptureMode: "document",
    voicePromptAutoRefine: true,
  });
  expect(reloaded.chatSessions.b.chatFilter).toBe("all");
});

it("patches legacy pending filters on hydration so re-enabling cannot restore them after reload", async () => {
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  const saved = {
    a: {
      chatFilter: "pending_user",
      chatSenderFilter: ["worker"],
      filesPanelOpen: true,
      extra: { fromAnotherTab: true },
    },
    b: { chatFilter: "mail", chatSenderFilter: ["leader"] },
  };
  localStorage.setItem(sessionsKey, JSON.stringify(saved));
  const { useUIStore } = await import("./useUIStore");
  expect(useUIStore.getState().chatSessions.a).toMatchObject({
    chatFilter: "all",
    chatSenderFilter: ["worker"],
    filesPanelOpen: true,
  });
  expect(useUIStore.getState().chatSessions.b.chatFilter).toBe("mail");
  expect(storedSessions()).toEqual({ ...saved, a: { ...saved.a, chatFilter: "all" } });
  useUIStore.getState().setShowPendingReplyReminders(true);
  vi.resetModules();
  const reloaded = (await import("./useUIStore")).useUIStore.getState();
  expect(reloaded.showPendingReplyReminders).toBe(true);
  expect(reloaded.chatSessions.a.chatFilter).toBe("all");
});

it("patches the latest persisted pending filters without overwriting another tab's sessions", async () => {
  const { useUIStore } = await import("./useUIStore");
  useUIStore.getState().setChatFilter("pending", "pending_user");
  useUIStore.getState().setChatFilter("other", "mail");
  useUIStore.getState().setChatSenderFilter("other", ["old-sender"]);
  const latest = {
    ...storedSessions(),
    other: {
      chatFilter: "user",
      chatSenderFilter: ["new-sender"],
      terminalPage: 7,
      extra: "preserved",
    },
    newGroup: { chatFilter: "pending_user", chatSenderFilter: ["peer"], extra: { value: 1 } },
  };
  localStorage.setItem(sessionsKey, JSON.stringify(latest));

  useUIStore.getState().setShowPendingReplyReminders(false);

  expect(useUIStore.getState().chatSessions.pending.chatFilter).toBe("all");
  expect(useUIStore.getState().chatSessions.other).toMatchObject({
    chatFilter: "mail",
    chatSenderFilter: ["old-sender"],
  });
  expect(storedSessions()).toEqual({
    ...latest,
    pending: { ...latest.pending, chatFilter: "all" },
    newGroup: { ...latest.newGroup, chatFilter: "all" },
  });
});

it("rejects stale pending-filter handlers while hidden and allows an explicit choice after enabling", async () => {
  const { useUIStore, getChatSession } = await import("./useUIStore");
  const staleSetFilter = useUIStore.getState().setChatFilter;
  staleSetFilter("a", "pending_user");
  useUIStore.getState().setShowPendingReplyReminders(false);
  staleSetFilter("a", "pending_user");
  staleSetFilter("new", "pending_user");
  expect(getChatSession("a", useUIStore.getState().chatSessions).chatFilter).toBe("all");
  expect(getChatSession("new", useUIStore.getState().chatSessions).chatFilter).toBe("all");
  expect(storedSessions().a.chatFilter).toBe("all");
  useUIStore.getState().setShowPendingReplyReminders(true);
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("all");
  staleSetFilter("a", "pending_user");
  expect(useUIStore.getState().chatSessions.a.chatFilter).toBe("pending_user");
});
