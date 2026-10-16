// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { useComposerStore, useGroupStore, useUIStore } from "../stores";
import { EMPTY_CHAT_BUCKET } from "../stores/groupStoreCore";
import { getChatSession } from "../stores/useUIStore";
import type { Actor, GroupMeta, LedgerEvent } from "../types";
import { useChatTab } from "./useChatTab";
import { ChatSenderFilter } from "../pages/chat/ChatSenderFilter";
import * as composerSendPlan from "./composerSendPlan";
import { useAppGroupLifecycle } from "./useAppGroupLifecycle";

vi.mock("../services/api", () => ({
  sendMessage: vi.fn(),
  sendCrossGroupMessage: vi.fn(),
  searchChatMessages: vi.fn(),
  fetchMessageWindow: vi.fn(),
}));
vi.mock("./useSlashCommandState", () => ({
  useSlashCommandState: () => ({ items: [], loading: false }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const actors = [
  { id: "leader", title: "Coordinator", runtime: "custom", role: "foreman", enabled: false },
] as Actor[];
const message = (id: string, by: string): LedgerEvent => ({
  id,
  by,
  kind: "chat.message",
  group_id: "a",
  ts: `2026-10-15T00:00:0${id}Z`,
  data: { text: `text ${id}`, to: ["user"], message_mode: "send" },
});
let root: Root;
let host: HTMLDivElement;
let chat: ReturnType<typeof useChatTab>;
const noop = () => undefined;
function Harness() {
  const selectedGroupId = useGroupStore((state) => state.selectedGroupId);
  chat = useChatTab({
    selectedGroupId,
    selectedGroupRunning: false,
    actors,
    recipientActors: actors,
  });
  useAppGroupLifecycle({
    selectedGroupId,
    destGroupId: selectedGroupId,
    sendGroupId: selectedGroupId,
    hasReplyTarget: false,
    setDestGroupId: noop,
    fileInputRef: { current: null },
    resetDragDrop: noop,
    setActiveTab: useUIStore.getState().setActiveTab,
    closeChatWindow: useGroupStore.getState().closeChatWindow,
    loadGroup: noop,
    connectStream: noop,
    cleanupSSE: noop,
  });
  return (
    <ChatSenderFilter
      value={chat.chatSenderFilter}
      actors={actors}
      onChange={chat.setChatSenderFilter}
    />
  );
}
beforeEach(async () => {
  vi.resetAllMocks();
  useComposerStore.setState({ ...useComposerStore.getInitialState() });
  useGroupStore.setState({
    selectedGroupId: "a",
    actors,
    groups: [
      { group_id: "a", title: "Test" },
      { group_id: "b", title: "Other" },
    ] as GroupMeta[],
    groupDoc: null,
    groupSettings: null,
    groupContext: null,
    chatByGroup: {
      a: {
        ...EMPTY_CHAT_BUCKET,
        hasLoadedTail: true,
        hasMoreHistory: true,
        events: [message("1", "worker")],
      },
    },
  });
  useUIStore.setState({
    chatSessions: {},
    busy: "",
    errorMsg: "",
    notice: null,
    activeTab: "chat",
  });
  useComposerStore.getState().switchGroup(null, "a");
  vi.mocked(api.searchChatMessages).mockResolvedValue({
    ok: true,
    result: { events: [message("2", "leader")], count: 1, has_more: false },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Harness />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useUIStore.getState().dismissNotice();
  vi.restoreAllMocks();
});

it("keeps sender history separate, then restores complete raw history on clear", async () => {
  const workBefore = chat.liveWorkEvents;
  await act(async () => chat.setChatSenderFilter(["leader"]));
  expect(chat.chatMessages.map((event) => event.id)).toEqual(["2"]);
  expect(chat.liveWorkEvents).toEqual(workBefore);
  expect(chat.suggestionSourceMessages.map((event) => event.id)).toEqual(["1"]);
  expect(useGroupStore.getState().chatByGroup.a.hasMoreHistory).toBe(true);
  await act(async () => chat.setChatSenderFilter([]));
  expect(chat.chatMessages.map((event) => event.id)).toEqual(["1"]);
  expect(chat.hasMoreHistory).toBe(true);
});

it("clears only the sender with its close button and keeps Mail selected", async () => {
  await act(async () => {
    chat.setChatFilter("mail");
    chat.setChatSenderFilter(["leader"]);
  });
  const clear = host.querySelector<HTMLButtonElement>('button[aria-label="clearSenderFilter"]');
  expect(clear).not.toBeNull();
  await act(async () => clear!.click());
  expect(chat).toMatchObject({ chatSenderFilter: [], chatFilter: "mail" });
  expect(getChatSession("a", useUIStore.getState().chatSessions)).toMatchObject({
    chatSenderFilter: [],
    chatFilter: "mail",
  });
  expect(host.querySelector('button[aria-label="clearSenderFilter"]')).toBeNull();
  expect(document.activeElement).toBe(host.querySelector('button[aria-haspopup="dialog"]'));
});

it("reads and displays a selected sender union without changing the raw Group cache", async () => {
  const events = [message("1", "worker"), message("2", "leader")];
  vi.mocked(api.searchChatMessages).mockResolvedValue({
    ok: true,
    result: { events, count: 2, has_more: false },
  });
  await act(async () => chat.setChatSenderFilter(["worker", "leader"]));
  expect(chat.chatMessages.map((event) => event.by)).toEqual(["worker", "leader"]);
  expect(api.searchChatMessages).toHaveBeenLastCalledWith(
    "a",
    "",
    { byAny: ["leader", "worker"], before: undefined, limit: 50 },
    expect.anything(),
  );
  expect(useGroupStore.getState().chatByGroup.a.events.map((event) => event.id)).toEqual(["1"]);
});

it("keeps filters when sending and offers the exact hidden message in full context", async () => {
  const sent = message("3", "user");
  vi.mocked(api.sendMessage).mockResolvedValue({ ok: true, result: { event: sent } });
  vi.mocked(api.fetchMessageWindow).mockResolvedValue({
    ok: true,
    result: {
      center_id: "3",
      center_index: 1,
      events: [message("2", "leader"), sent],
      count: 2,
      has_more_before: false,
      has_more_after: false,
    },
  });
  await act(async () => {
    chat.setChatSenderFilter(["leader"]);
    useComposerStore.getState().setComposerText("hello");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(chat.chatSenderFilter).toEqual(["leader"]);
  expect(chat.chatMessages.map((event) => event.by)).toEqual(["leader"]);
  expect(chat.sendScrollRequest).toBeNull();
  const notice = useUIStore.getState().notice;
  expect(notice).toMatchObject({
    message: "sentOutsideChatFilter",
    actionLabel: "viewSentMessage",
  });
  await act(async () => notice?.onAction?.());
  expect(api.fetchMessageWindow).toHaveBeenCalledWith("a", "3", { before: 30, after: 30 });
  expect(chat.inChatWindow).toBe(true);
  expect(chat.chatMessages.map((event) => event.by)).toEqual(["leader", "user"]);
  await act(async () => chat.exitChatWindow());
  expect(chat.chatMessages.map((event) => event.by)).toEqual(["leader"]);
  expect(getChatSession("a", useUIStore.getState().chatSessions).chatSenderFilter).toEqual([
    "leader",
  ]);
});

it("does not change routing or reset the message category when sending within a filter", async () => {
  const sent = message("3", "user");
  vi.mocked(api.sendMessage).mockResolvedValue({ ok: true, result: { event: sent } });
  await act(async () => {
    chat.setChatSenderFilter(["user"]);
    chat.setChatFilter("user");
    useComposerStore.getState().setComposerText("hello");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(chat).toMatchObject({ chatSenderFilter: ["user"], chatFilter: "user" });
  expect(api.sendMessage).toHaveBeenCalledTimes(1);
  expect(useUIStore.getState().notice).toBeNull();
});

it("resolves the active Group when a persistent hidden-send notice is followed", async () => {
  const sent = message("3", "user");
  vi.mocked(api.sendMessage).mockResolvedValue({ ok: true, result: { event: sent } });
  await act(async () => {
    chat.setChatSenderFilter(["leader"]);
    useComposerStore.getState().setComposerText("hello");
  });
  await act(async () => chat.sendMessage());
  const notice = useUIStore.getState().notice;
  vi.mocked(api.fetchMessageWindow).mockResolvedValue({
    ok: true,
    result: {
      center_id: "3",
      center_index: 0,
      events: [sent],
      count: 1,
      has_more_before: false,
      has_more_after: false,
    },
  });
  await act(async () => useGroupStore.getState().setSelectedGroupId("b"));
  expect(useGroupStore.getState().selectedGroupId).toBe("b");
  await act(async () => notice?.onAction?.());
  expect(useGroupStore.getState().selectedGroupId).toBe("a");
  expect(new URL(window.location.href).searchParams.get("group")).toBe("a");
  expect(new URL(window.location.href).searchParams.get("event")).toBe("3");
  expect(api.fetchMessageWindow).toHaveBeenCalledWith("a", "3", { before: 30, after: 30 });
  expect(chat.inChatWindow).toBe(true);
  expect(chat.chatMessages.map((event) => event.id)).toEqual(["3"]);
  expect(getChatSession("a", useUIStore.getState().chatSessions).chatSenderFilter).toEqual([
    "leader",
  ]);
});

it.each(["source_event", "src_event"])(
  "confirms a filtered cross-Group send using its %s in the originating Group",
  async (field) => {
    const source = {
      ...message("3", "user"),
      data: { text: "cross send", message_mode: "send" as const, dst_group_id: "b" },
    };
    vi.mocked(api.sendCrossGroupMessage).mockResolvedValue({
      ok: true,
      result: { [field]: source },
    });
    vi.mocked(api.fetchMessageWindow).mockResolvedValue({
      ok: true,
      result: {
        center_id: "3",
        center_index: 0,
        events: [source],
        count: 1,
        has_more_before: false,
        has_more_after: false,
      },
    });
    await act(async () => {
      chat.setChatSenderFilter(["leader"]);
      useComposerStore.getState().setDestGroupId("b");
      useComposerStore.getState().setComposerText("hello other Group");
    });
    await act(async () => chat.sendMessage());
    expect(api.sendCrossGroupMessage).toHaveBeenCalledTimes(1);
    expect(api.sendMessage).not.toHaveBeenCalled();
    expect(useComposerStore.getState().composerText).toBe("");
    expect(chat.chatSenderFilter).toEqual(["leader"]);
    const notice = useUIStore.getState().notice;
    expect(notice).toMatchObject({
      message: "sentOutsideChatFilter",
      actionLabel: "viewSentMessage",
    });
    await act(async () => notice?.onAction?.());
    expect(api.fetchMessageWindow).toHaveBeenCalledWith("a", "3", { before: 30, after: 30 });
  },
);

it("confirms the originating message after a mixed local and cross-Group send", async () => {
  vi.spyOn(composerSendPlan, "buildComposerSendPlanTargets").mockReturnValueOnce([
    { groupId: "a", isCrossGroup: false, source: "selected_group" },
    { groupId: "b", isCrossGroup: true, source: "group_mention" },
  ]);
  vi.mocked(api.sendMessage).mockResolvedValue({
    ok: true,
    result: { event: message("3", "user") },
  });
  vi.mocked(api.sendCrossGroupMessage).mockResolvedValue({
    ok: true,
    result: {
      source_event: message("4", "user"),
      event: { ...message("destination", "user"), group_id: "b" },
    },
  });
  vi.mocked(api.fetchMessageWindow).mockResolvedValue({
    ok: true,
    result: {
      center_id: "4",
      center_index: 0,
      events: [message("4", "user")],
      count: 1,
      has_more_before: false,
      has_more_after: false,
    },
  });
  await act(async () => {
    chat.setChatSenderFilter(["leader"]);
    useComposerStore.getState().setComposerText("hello both");
  });
  await act(async () => chat.sendMessage());
  expect(api.sendMessage).toHaveBeenCalledTimes(1);
  expect(api.sendCrossGroupMessage).toHaveBeenCalledTimes(1);
  const notice = useUIStore.getState().notice;
  expect(notice?.actionLabel).toBe("viewSentMessage");
  await act(async () => notice?.onAction?.());
  expect(api.fetchMessageWindow).toHaveBeenCalledWith("a", "4", { before: 30, after: 30 });
});

it("renders the filtered conversation in ledger order despite clock rollback", async () => {
  const events = [
    message("2", "leader"),
    { ...message("3", "leader"), ts: "2026-10-14T00:00:00Z" },
  ];
  vi.mocked(api.searchChatMessages).mockResolvedValue({
    ok: true,
    result: { events, count: 2, has_more: false },
  });
  await act(async () => chat.setChatSenderFilter(["leader"]));
  expect(chat.chatMessages.map((event) => event.id)).toEqual(["2", "3"]);
});
