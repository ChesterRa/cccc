// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useUIStore } from "../../stores/useUIStore";
import { useGroupStore } from "../../stores/useGroupStore";
import {
  finishWorkspaceNavigation,
  setWorkspaceDirty,
  useWorkspaceNavigation,
} from "../../stores/workspaceNavigation";
import { PENDING_REPLY_REMINDERS_KEY } from "../../utils/pendingReplyReminders";
import { openPendingReplies } from "./navigation";

const original = useUIStore.getState();
const originalGroups = useGroupStore.getState();
beforeEach(() => {
  localStorage.clear();
  useUIStore.setState({
    showPendingPrompts: true,
    showPendingReplyReminders: true,
    chatSessions: {},
    activeTab: "settings",
  });
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  useUIStore.getState().setChatSenderFilter("b", ["worker"]);
  useUIStore.getState().setChatFilter("b", "mail");
});
afterEach(() => {
  vi.restoreAllMocks();
  useUIStore.setState(original);
  useGroupStore.setState(originalGroups);
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  localStorage.clear();
});

it("honors a page-local enable when persistent writes fail but reads still return false", () => {
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  const write = vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  useUIStore.getState().setShowPendingReplyReminders(true);
  const select = vi.fn(),
    close = vi.fn();
  openPendingReplies("b", select, close);
  expect(select).toHaveBeenCalledWith("b");
  expect(close).toHaveBeenCalledOnce();
  expect(useUIStore.getState().chatSessions.b.chatFilter).toBe("pending_user");
  write.mockRestore();
  warn.mockRestore();
  useUIStore.getState().setShowPendingReplyReminders(true);
});

it("abandons a deferred navigation when another tab hides reminders before its event arrives", () => {
  setWorkspaceDirty(Symbol("editor"), true);
  const select = vi.fn(),
    close = vi.fn();
  openPendingReplies("b", select, close);
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  const before = useUIStore.getState().chatSessions;
  // Shared storage changes before this tab receives the asynchronous storage event.
  localStorage.setItem(PENDING_REPLY_REMINDERS_KEY, "false");
  finishWorkspaceNavigation(true);
  expect(select).not.toHaveBeenCalled();
  expect(close).not.toHaveBeenCalled();
  expect(useUIStore.getState().chatSessions).toBe(before);
  expect(useUIStore.getState().activeTab).toBe("settings");
});

it("abandons deferred pending navigation when upstream quiet mode is enabled", () => {
  setWorkspaceDirty(Symbol("editor"), true);
  const select = vi.fn(),
    close = vi.fn();
  openPendingReplies("b", select, close);
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  useUIStore.getState().setShowPendingPrompts(false);
  finishWorkspaceNavigation(true);
  expect(select).not.toHaveBeenCalled();
  expect(close).not.toHaveBeenCalled();
  expect(useUIStore.getState().activeTab).toBe("settings");
  expect(useUIStore.getState().chatSessions.b.chatFilter).toBe("mail");
});

it("does not open a confirmation while hidden, and retains the ordinary navigation when shown", () => {
  const select = vi.fn(),
    close = vi.fn();
  useUIStore.getState().setShowPendingReplyReminders(false);
  setWorkspaceDirty(Symbol("editor"), true);
  openPendingReplies("b", select, close);
  expect(useWorkspaceNavigation.getState().pending).toBeNull();
  useUIStore.getState().setShowPendingReplyReminders(true);
  openPendingReplies("b", select, close);
  finishWorkspaceNavigation(true);
  expect(select).toHaveBeenCalledWith("b");
  expect(close).toHaveBeenCalledOnce();
  expect(useUIStore.getState().chatSessions.b).toMatchObject({
    chatFilter: "pending_user",
    chatSenderFilter: [],
    workView: "messages",
    mobileSurface: "messages",
  });
  expect(useUIStore.getState().activeTab).toBe("chat");
});
