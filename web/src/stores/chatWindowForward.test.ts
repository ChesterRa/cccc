// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import type { LedgerEvent } from "../types";
import { EMPTY_CHAT_BUCKET } from "./groupStoreCore";
import { useGroupStore } from "./useGroupStore";

vi.mock("../services/api", () => ({ fetchMessageWindow: vi.fn() }));
const message = (id: string): LedgerEvent => ({
  id,
  kind: "chat.message",
  by: "worker",
  group_id: "a",
  ts: `2026-01-01T00:00:0${id}Z`,
  data: { text: id },
});
const windowFor = (center = "2") => ({
  groupId: "a",
  centerEventId: center,
  centerIndex: 1,
  events: [message("1"), message("2"), message("3")],
  hasMoreBefore: false,
  hasMoreAfter: true,
});
const result = (events: LedgerEvent[], hasMore = false) => ({
  ok: true as const,
  result: {
    center_id: "3",
    center_index: 0,
    events,
    count: events.length,
    has_more_before: true,
    has_more_after: hasMore,
  },
});
beforeEach(() => {
  vi.resetAllMocks();
  useGroupStore.setState({
    selectedGroupId: "a",
    chatByGroup: {
      a: { ...EMPTY_CHAT_BUCKET, chatWindow: windowFor(), isChatWindowLoading: false },
    },
  });
});

describe("newer context pages", () => {
  it("appends one chronological page, deduplicates its center, and keeps its original target", async () => {
    vi.mocked(api.fetchMessageWindow).mockResolvedValue(
      result([message("3"), message("4"), message("5")], true),
    );
    await useGroupStore.getState().loadMoreChatWindowAfter("a");
    expect(api.fetchMessageWindow).toHaveBeenCalledWith("a", "3", { before: 0, after: 50 });
    expect(useGroupStore.getState().chatByGroup.a.chatWindow).toMatchObject({
      centerEventId: "2",
      hasMoreAfter: true,
      isLoadingAfter: false,
      afterError: false,
    });
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.events.map((e) => e.id)).toEqual([
      "1",
      "2",
      "3",
      "4",
      "5",
    ]);
  });
  it("allows only one page request at a time", async () => {
    let finish!: (value: ReturnType<typeof result>) => void;
    vi.mocked(api.fetchMessageWindow).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const first = useGroupStore.getState().loadMoreChatWindowAfter("a");
    const second = useGroupStore.getState().loadMoreChatWindowAfter("a");
    expect(api.fetchMessageWindow).toHaveBeenCalledTimes(1);
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.isLoadingAfter).toBe(true);
    finish(result([message("3"), message("4")]));
    await Promise.all([first, second]);
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.events.map((e) => e.id)).toEqual([
      "1",
      "2",
      "3",
      "4",
    ]);
  });
  it("shows a retryable error and retains all existing messages", async () => {
    vi.mocked(api.fetchMessageWindow)
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(result([message("3"), message("4")]));
    await useGroupStore.getState().loadMoreChatWindowAfter("a");
    expect(useGroupStore.getState().chatByGroup.a.chatWindow).toMatchObject({
      afterError: true,
      isLoadingAfter: false,
      hasMoreAfter: true,
    });
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.events.map((e) => e.id)).toEqual([
      "1",
      "2",
      "3",
    ]);
    await useGroupStore.getState().loadMoreChatWindowAfter("a");
    expect(useGroupStore.getState().chatByGroup.a.chatWindow).toMatchObject({
      afterError: false,
      isLoadingAfter: false,
      hasMoreAfter: false,
    });
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.events.map((e) => e.id)).toEqual([
      "1",
      "2",
      "3",
      "4",
    ]);
  });
  it("does not apply an old page after the user closes the context", async () => {
    let finish!: (value: ReturnType<typeof result>) => void;
    vi.mocked(api.fetchMessageWindow).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const waiting = useGroupStore.getState().loadMoreChatWindowAfter("a");
    useGroupStore.getState().closeChatWindow("a");
    finish(result([message("3"), message("4")]));
    await waiting;
    expect(useGroupStore.getState().chatByGroup.a.chatWindow).toBeNull();
  });
  it("does not apply an old page to a new context with the same Group", async () => {
    let finish!: (value: ReturnType<typeof result>) => void;
    vi.mocked(api.fetchMessageWindow)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          finish = resolve;
        }),
      )
      .mockResolvedValueOnce(result([message("3"), message("4")]));
    const waiting = useGroupStore.getState().loadMoreChatWindowAfter("a");
    await useGroupStore.getState().openChatWindow("a", "3");
    finish(result([message("3"), message("5")]));
    await waiting;
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.centerEventId).toBe("3");
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.events.map((e) => e.id)).toEqual([
      "3",
      "4",
    ]);
  });
  it("discards a late page after switching away and back, while allowing a retry", async () => {
    let finish!: (value: ReturnType<typeof result>) => void;
    vi.mocked(api.fetchMessageWindow)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          finish = resolve;
        }),
      )
      .mockResolvedValueOnce(result([message("3"), message("4")]));
    const waiting = useGroupStore.getState().loadMoreChatWindowAfter("a");
    useGroupStore.getState().setSelectedGroupId("b");
    useGroupStore.getState().setSelectedGroupId("a");
    finish(result([message("3"), message("5")]));
    await waiting;
    expect(useGroupStore.getState().chatByGroup.a.chatWindow).toMatchObject({
      isLoadingAfter: false,
      hasMoreAfter: true,
    });
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.events.map((e) => e.id)).toEqual([
      "1",
      "2",
      "3",
    ]);
    await useGroupStore.getState().loadMoreChatWindowAfter("a");
    expect(useGroupStore.getState().chatByGroup.a.chatWindow?.events.map((e) => e.id)).toEqual([
      "1",
      "2",
      "3",
      "4",
    ]);
  });
  it("fails visibly instead of accepting a response without progress", async () => {
    vi.mocked(api.fetchMessageWindow).mockResolvedValue(result([message("3")], true));
    await useGroupStore.getState().loadMoreChatWindowAfter("a");
    expect(useGroupStore.getState().chatByGroup.a.chatWindow).toMatchObject({
      afterError: true,
      hasMoreAfter: true,
    });
  });
});
