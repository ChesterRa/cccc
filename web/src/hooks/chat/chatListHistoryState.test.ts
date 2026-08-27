import { describe, expect, it } from "vite-plus/test";

import { resolveChatListHistoryState } from "./chatListHistoryState";

const base = {
  selectedGroupId: "group-1",
  chatFilter: "all" as const,
  filteredMessageCount: 0,
  hasAnyChatMessages: false,
  inChatWindow: false,
  hasLoadedTail: true,
  hasMoreHistory: true,
  isLoadingHistory: false,
  isChatWindowLoading: false,
};

describe("resolveChatListHistoryState", () => {
  it("renders an empty filtered view without inheriting group pagination", () => {
    expect(
      resolveChatListHistoryState({ ...base, chatFilter: "mail", hasAnyChatMessages: true }),
    ).toEqual({ isLoadingHistory: false, hasMoreHistory: false });
  });

  it("keeps real initial history loading when no base messages are available", () => {
    expect(
      resolveChatListHistoryState({
        ...base,
        chatFilter: "mail",
        hasLoadedTail: false,
        isLoadingHistory: true,
      }),
    ).toEqual({ isLoadingHistory: true, hasMoreHistory: true });
  });

  it("keeps centered chat windows bounded to their own loading state", () => {
    expect(
      resolveChatListHistoryState({ ...base, inChatWindow: true, isChatWindowLoading: true }),
    ).toEqual({ isLoadingHistory: true, hasMoreHistory: false });
  });
});
