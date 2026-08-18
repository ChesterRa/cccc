export const CHAT_SEND_MAX_REMAINING_MESSAGE_BLOCKS = 2;

export interface MessageRowViewportMetric {
  index: number;
  top: number;
  bottom: number;
}

export function getLastVisibleMessageIndex(input: {
  viewportTop: number;
  viewportBottom: number;
  rows: MessageRowViewportMetric[];
}): number | null {
  const viewportTop = Number(input.viewportTop) || 0;
  const viewportBottom = Number(input.viewportBottom) || 0;
  let lastVisibleIndex: number | null = null;
  for (const row of input.rows) {
    const index = Number(row.index);
    if (!Number.isInteger(index) || index < 0) continue;
    if (!(Number(row.bottom) > viewportTop + 1 && Number(row.top) < viewportBottom - 1)) continue;
    lastVisibleIndex = lastVisibleIndex == null ? index : Math.max(lastVisibleIndex, index);
  }
  return lastVisibleIndex;
}

export function shouldFollowChatSend(input: {
  messageCount: number;
  lastVisibleMessageIndex: number | null;
  maxRemainingBlocks?: number;
}): boolean {
  const messageCount = Math.max(0, Math.floor(Number(input.messageCount) || 0));
  if (messageCount === 0) return true;
  const lastVisibleMessageIndex = input.lastVisibleMessageIndex;
  if (
    lastVisibleMessageIndex == null ||
    !Number.isInteger(lastVisibleMessageIndex) ||
    lastVisibleMessageIndex < 0 ||
    lastVisibleMessageIndex >= messageCount
  ) {
    return false;
  }
  const maxRemainingBlocks = Math.max(
    0,
    Math.floor(Number(input.maxRemainingBlocks ?? CHAT_SEND_MAX_REMAINING_MESSAGE_BLOCKS) || 0),
  );
  return messageCount - lastVisibleMessageIndex - 1 <= maxRemainingBlocks;
}

export function shouldFollowChatSendFromViewport(
  container: HTMLElement | null | undefined,
  messageCount: number,
): boolean {
  if (messageCount <= 0) return true;
  if (!container) return false;
  const viewport = container.getBoundingClientRect();
  const rows = Array.from(
    container.querySelectorAll<HTMLElement>('[data-message-row="true"][data-index]'),
  ).map((row) => {
    const rect = row.getBoundingClientRect();
    return { index: Number(row.dataset.index), top: rect.top, bottom: rect.bottom };
  });
  const lastVisibleMessageIndex = getLastVisibleMessageIndex({
    viewportTop: viewport.top,
    viewportBottom: viewport.bottom,
    rows,
  });
  return shouldFollowChatSend({ messageCount, lastVisibleMessageIndex });
}
