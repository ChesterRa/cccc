import type { ChatFilter } from "../../stores/useUIStore";

export function resolveChatListHistoryState(input: {
  selectedGroupId: string;
  chatFilter: ChatFilter;
  filteredMessageCount: number;
  hasAnyChatMessages: boolean;
  inChatWindow: boolean;
  hasLoadedTail: boolean;
  hasMoreHistory: boolean;
  isLoadingHistory: boolean;
  isChatWindowLoading: boolean;
}): { isLoadingHistory: boolean; hasMoreHistory: boolean; isFilteredEmpty: boolean } {
  if (!input.selectedGroupId) {
    return { isLoadingHistory: false, hasMoreHistory: false, isFilteredEmpty: false };
  }
  if (input.inChatWindow) {
    return {
      isLoadingHistory: input.isChatWindowLoading,
      hasMoreHistory: false,
      isFilteredEmpty: false,
    };
  }
  const isFilteredEmpty =
    input.hasLoadedTail &&
    input.chatFilter !== "all" &&
    input.filteredMessageCount <= 0 &&
    input.hasAnyChatMessages;
  return {
    isLoadingHistory: input.isLoadingHistory,
    hasMoreHistory: !input.hasLoadedTail || input.hasMoreHistory,
    isFilteredEmpty,
  };
}
