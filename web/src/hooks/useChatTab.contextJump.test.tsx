// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useGroupStore, useUIStore } from "../stores";
import {
  CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
  type ChatScrollSnapshot,
} from "../stores/useUIStore";
import { EMPTY_CHAT_BUCKET } from "../stores/groupStoreCore";
import type { Actor, GroupMeta, LedgerEvent } from "../types";
import { useChatTab } from "./useChatTab";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./useSlashCommandState", () => ({
  useSlashCommandState: () => ({ items: [], loading: false }),
}));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const message = (id: string): LedgerEvent => ({
  id,
  by: "worker",
  kind: "chat.message",
  group_id: "a",
  ts: `2026-01-01T00:00:0${id}Z`,
  data: { text: `message ${id}`, to: ["user"], message_mode: "send" },
});
const groupWindow = () => ({
  groupId: "a",
  centerEventId: "2",
  centerIndex: 1,
  events: [message("1"), message("2"), message("3")],
  hasMoreBefore: false,
  hasMoreAfter: false,
});
let host: HTMLDivElement;
let log: HTMLDivElement;
let root: Root;
let chat: ReturnType<typeof useChatTab>;
const scrollRef: { current: HTMLDivElement | null } = { current: null };
function Harness() {
  const selectedGroupId = useGroupStore((state) => state.selectedGroupId);
  chat = useChatTab({
    selectedGroupId,
    selectedGroupRunning: false,
    actors: [] as Actor[],
    recipientActors: [] as Actor[],
    scrollRef,
  });
  return null;
}
beforeEach(async () => {
  const win = groupWindow();
  useGroupStore.setState({
    selectedGroupId: "a",
    actors: [],
    groups: [
      { group_id: "a", title: "A" },
      { group_id: "b", title: "B" },
    ] as GroupMeta[],
    groupDoc: null,
    groupSettings: null,
    groupContext: null,
    chatByGroup: {
      a: {
        ...EMPTY_CHAT_BUCKET,
        events: [message("1"), message("2"), message("3")],
        chatWindow: win,
        hasLoadedTail: true,
        isChatWindowLoading: false,
      },
    },
  });
  useUIStore.setState({
    chatSessions: {},
    activeTab: "chat",
    busy: "",
    errorMsg: "",
    notice: null,
    isSmallScreen: false,
  });
  history.replaceState({}, "", "/?group=a&event=2&tab=chat");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  log = document.createElement("div");
  log.setAttribute("role", "log");
  log.getBoundingClientRect = () => ({ top: 100, bottom: 500 }) as DOMRect;
  const row = document.createElement("div");
  row.dataset.messageRow = "true";
  row.dataset.messageId = "2";
  row.getBoundingClientRect = () => ({ top: 240, bottom: 300 }) as DOMRect;
  log.append(row);
  document.body.append(log);
  scrollRef.current = log;
  await act(async () => root.render(<Harness />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  log.remove();
  scrollRef.current = null;
  history.replaceState({}, "", "/");
  vi.restoreAllMocks();
});

it("moves a loaded target to live chat at its reading top and keeps new events visible", async () => {
  expect(chat.onContextReady).toBeTypeOf("function");
  await act(async () => chat.onContextReady?.());
  expect(chat.inChatWindow).toBe(false);
  expect(chat.chatInitialScrollAnchorId).toBe("2");
  // Not the context's own height: on a phone the live list starts under the chrome.
  expect(chat.chatInitialScrollAnchorOffsetPx).toBeUndefined();
  expect(chat.chatInitialScrollHighlight).toBe(true);
  expect(new URLSearchParams(location.search).has("event")).toBe(false);
  await act(async () => useGroupStore.getState().appendEvent(message("4"), "a"));
  expect(chat.chatMessages.map((event) => event.id)).toContain("4");
});

it("moves a loaded target to live chat under a stored Waiting for you filter it does not show", async () => {
  // No Waiting for you tab is offered here, as in an exhibit or with those prompts off, so the
  // stored filter shows every message and the opened target joins the live list as usual.
  await act(async () => useUIStore.getState().setChatFilter("a", "pending_user"));
  expect(chat.chatFilter).toBe("all");
  expect(chat.onContextReady).toBeTypeOf("function");
  await act(async () => chat.onContextReady?.());
  expect(chat.inChatWindow).toBe(false);
  expect(chat.chatInitialScrollAnchorId).toBe("2");
});

it("does not turn an older target absent from the live tail into a jump", async () => {
  await act(async () =>
    useGroupStore.setState((state) => ({
      chatByGroup: { ...state.chatByGroup, a: { ...state.chatByGroup.a, events: [message("3")] } },
    })),
  );
  expect(chat.onContextReady).toBeUndefined();
  expect(chat.inChatWindow).toBe(true);
});

it("does not close a context after its Group changes", async () => {
  const ready = chat.onContextReady!;
  await act(async () => {
    useGroupStore.getState().setSelectedGroupId("b");
    ready();
  });
  expect(useGroupStore.getState().chatByGroup.a.chatWindow?.centerEventId).toBe("2");
  expect(new URLSearchParams(location.search).get("event")).toBe("2");
});

it("does not close a context if a filter is enabled before the callback runs", async () => {
  const ready = chat.onContextReady!;
  await act(async () => {
    useUIStore.getState().setChatFilter("a", "user");
    ready();
  });
  expect(useGroupStore.getState().chatByGroup.a.chatWindow?.centerEventId).toBe("2");
  expect(new URLSearchParams(location.search).get("event")).toBe("2");
});

it("does not close a newer request when a late callback belongs to the old window", async () => {
  const ready = chat.onContextReady!;
  await act(async () => {
    useGroupStore.getState().setChatWindow({ ...groupWindow(), centerEventId: "3" }, "a");
    ready();
  });
  expect(useGroupStore.getState().chatByGroup.a.chatWindow?.centerEventId).toBe("3");
});

it("opens an already loaded message without waiting for a context fetch", async () => {
  await act(async () =>
    useGroupStore.setState((state) => ({
      chatByGroup: {
        ...state.chatByGroup,
        a: {
          ...state.chatByGroup.a,
          chatWindow: { ...groupWindow(), events: [] },
          isChatWindowLoading: true,
        },
      },
    })),
  );
  expect(chat.inChatWindow).toBe(false);
  expect(chat.chatInitialScrollAnchorId).toBe("2");
  expect(useGroupStore.getState().chatByGroup.a.isChatWindowLoading).toBe(false);
});

it("opens a loaded message at the reading top, not at another place the chat saved", async () => {
  await act(async () =>
    useUIStore
      .getState()
      .setChatScrollSnapshot("a", {
        coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
        mode: "detached",
        anchorId: "1",
        offsetPx: 55,
        scrollTop: 400,
        updatedAt: Date.now(),
      }),
  );
  await act(async () =>
    useGroupStore.setState((state) => ({
      chatByGroup: {
        ...state.chatByGroup,
        a: {
          ...state.chatByGroup.a,
          chatWindow: { ...groupWindow(), events: [] },
          isChatWindowLoading: true,
        },
      },
    })),
  );
  expect(chat.chatInitialScrollAnchorId).toBe("2");
  // No height of its own: the list opens it at the reading top, below any chrome over it.
  expect(chat.chatInitialScrollAnchorOffsetPx).toBeUndefined();
});

it("returns from an older context without acknowledging unread messages until live bottom", async () => {
  await act(async () => {
    useGroupStore.setState((state) => ({
      chatByGroup: {
        ...state.chatByGroup,
        a: {
          ...state.chatByGroup.a,
          events: [message("3")],
          chatWindow: { ...groupWindow(), hasMoreAfter: true },
        },
      },
    }));
    useUIStore.getState().setChatUnreadCount("a", 2);
  });
  await act(async () => chat.handleScrollChange(true));
  expect(useUIStore.getState().chatSessions.a.chatUnreadCount).toBe(2);
  expect(chat.showScrollButton).toBe(true);
  await act(async () => chat.handleScrollButtonClick());
  expect(chat.inChatWindow).toBe(false);
  expect(useUIStore.getState().chatSessions.a.chatUnreadCount).toBe(2);
  expect(useUIStore.getState().chatSessions.a.scrollSnapshot?.mode).toBe("follow");
  expect(new URLSearchParams(location.search).has("event")).toBe(false);
  await act(async () => chat.handleScrollChange(true));
  expect(useUIStore.getState().chatSessions.a.chatUnreadCount).toBe(0);
});

it("does not return a stale window after the selected Group changes", async () => {
  const back = chat.returnToLatest;
  await act(async () => {
    useGroupStore.getState().setSelectedGroupId("b");
    back();
  });
  expect(useGroupStore.getState().chatByGroup.a.chatWindow?.centerEventId).toBe("2");
  expect(new URLSearchParams(location.search).get("event")).toBe("2");
});

// A message opened from a filtered view is read and left again; the filtered view reopens at
// the place it was read, while Return to latest still opens the newest message.
it("returns to a filtered conversation at the place it was read", async () => {
  const place: ChatScrollSnapshot = {
    coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
    mode: "detached",
    anchorId: "1",
    offsetPx: 12,
    updatedAt: Date.now(),
  };
  await act(async () => {
    useUIStore.getState().setChatFilter("a", "user");
    useUIStore.getState().setChatScrollSnapshot("a", place);
  });
  await act(async () => chat.returnToLatest(false, true));
  expect(chat.inChatWindow).toBe(false);
  expect(useUIStore.getState().chatSessions.a.scrollSnapshot).toMatchObject({
    mode: "detached",
    anchorId: "1",
    offsetPx: 12,
  });
  expect(new URLSearchParams(location.search).has("event")).toBe(false);
});

it("returns to the same filter without acknowledging unread messages early", async () => {
  await act(async () => {
    useUIStore.getState().setChatFilter("a", "user");
    useUIStore.getState().setChatUnreadCount("a", 2);
  });
  await act(async () => chat.returnToLatest());
  expect(chat.inChatWindow).toBe(false);
  expect(chat.chatFilter).toBe("user");
  expect(useUIStore.getState().chatSessions.a.chatUnreadCount).toBe(2);
});
