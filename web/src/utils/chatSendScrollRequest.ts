export interface ChatSendScrollRequest {
  requestId: number;
  groupId: string;
  viewKey: string;
}

export function createChatSendScrollRequest(
  requestId: number,
  groupId: string,
  viewKey: string,
): ChatSendScrollRequest {
  return {
    requestId: Math.max(1, Math.floor(Number(requestId) || 0)),
    groupId: String(groupId || ""),
    viewKey: String(viewKey || ""),
  };
}

export function isChatSendScrollRequestOwner(
  request: ChatSendScrollRequest | null | undefined,
  groupId: string,
  viewKey: string,
): boolean {
  return (
    !!request &&
    request.groupId === String(groupId || "") &&
    request.viewKey === String(viewKey || "")
  );
}

export function consumeChatSendScrollRequest(
  current: ChatSendScrollRequest | null,
  requestId: number,
): ChatSendScrollRequest | null {
  return current?.requestId === requestId ? null : current;
}

export function invalidateChatSendScrollRequestForOwner(
  current: ChatSendScrollRequest | null,
  groupId: string,
  viewKey: string,
): ChatSendScrollRequest | null {
  return isChatSendScrollRequestOwner(current, groupId, viewKey) ? current : null;
}

export function shouldExecuteChatSendScroll(input: {
  request: ChatSendScrollRequest;
  activeRequest: ChatSendScrollRequest | null;
  requestToken: number;
  currentRequestToken: number;
  groupId: string;
  viewKey: string;
}): boolean {
  return (
    input.requestToken === input.currentRequestToken &&
    input.activeRequest?.requestId === input.request.requestId &&
    isChatSendScrollRequestOwner(input.request, input.groupId, input.viewKey)
  );
}
