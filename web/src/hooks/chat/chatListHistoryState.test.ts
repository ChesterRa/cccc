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
  it("keeps older history reachable from an empty filtered tail", () => {
    expect(
      resolveChatListHistoryState({ ...base, chatFilter: "mail", hasAnyChatMessages: true }),
    ).toEqual({ isLoadingHistory: false, hasMoreHistory: true, isFilteredEmpty: true });
  });

  it("keeps real initial history loading even if live events arrive first", () => {
    expect(
      resolveChatListHistoryState({
        ...base,
        chatFilter: "mail",
        hasAnyChatMessages: true,
        hasLoadedTail: false,
        isLoadingHistory: true,
      }),
    ).toEqual({ isLoadingHistory: true, hasMoreHistory: true, isFilteredEmpty: false });
  });

  it("keeps centered chat windows bounded to their own loading state", () => {
    expect(
      resolveChatListHistoryState({ ...base, inChatWindow: true, isChatWindowLoading: true }),
    ).toEqual({ isLoadingHistory: true, hasMoreHistory: false, isFilteredEmpty: false });
  });

  it("settles an exhausted filtered history as a real empty result", () => {
    expect(
      resolveChatListHistoryState({
        ...base,
        chatFilter: "request_reply",
        hasAnyChatMessages: true,
        hasMoreHistory: false,
      }),
    ).toEqual({ isLoadingHistory: false, hasMoreHistory: false, isFilteredEmpty: true });
  });
});
