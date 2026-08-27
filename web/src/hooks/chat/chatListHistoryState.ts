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
}): { isLoadingHistory: boolean; hasMoreHistory: boolean } {
  if (!input.selectedGroupId) {
    return { isLoadingHistory: false, hasMoreHistory: false };
  }
  if (input.inChatWindow) {
    return { isLoadingHistory: input.isChatWindowLoading, hasMoreHistory: false };
  }
  const filteredEmpty =
    input.chatFilter !== "all" && input.filteredMessageCount <= 0 && input.hasAnyChatMessages;
  if (filteredEmpty) {
    return { isLoadingHistory: false, hasMoreHistory: false };
  }
  return {
    isLoadingHistory: input.isLoadingHistory,
    hasMoreHistory: !input.hasLoadedTail || input.hasMoreHistory,
  };
}
