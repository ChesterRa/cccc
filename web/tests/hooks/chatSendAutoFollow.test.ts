import { describe, expect, it } from "vite-plus/test";

import {
  getLastVisibleMessageIndex,
  shouldFollowChatSend,
  shouldFollowChatSendFromViewport,
} from "../../src/hooks/chat/chatSendAutoFollow";

describe("chat send message-block follow intent", () => {
  it("allows 0, 1, or 2 remaining blocks and rejects 3", () => {
    expect(shouldFollowChatSend({ messageCount: 10, lastVisibleMessageIndex: 9 })).toBe(true);
    expect(shouldFollowChatSend({ messageCount: 10, lastVisibleMessageIndex: 8 })).toBe(true);
    expect(shouldFollowChatSend({ messageCount: 10, lastVisibleMessageIndex: 7 })).toBe(true);
    expect(shouldFollowChatSend({ messageCount: 10, lastVisibleMessageIndex: 6 })).toBe(false);
  });

  it("uses the last truly visible row and ignores virtualizer overscan", () => {
    const lastVisibleMessageIndex = getLastVisibleMessageIndex({
      viewportTop: 0,
      viewportBottom: 600,
      rows: [
        { index: 5, top: -180, bottom: 120 },
        { index: 6, top: 120, bottom: 360 },
        { index: 7, top: 360, bottom: 590 },
        { index: 8, top: 640, bottom: 780 },
        { index: 9, top: 780, bottom: 920 },
      ],
    });

    expect(lastVisibleMessageIndex).toBe(7);
    expect(shouldFollowChatSend({ messageCount: 10, lastVisibleMessageIndex })).toBe(true);
  });

  it("reads data-index from rendered rows without treating overscan as visible", () => {
    const row = (index: number, top: number, bottom: number) =>
      ({
        dataset: { index: String(index) },
        getBoundingClientRect: () => ({ top, bottom }),
      }) as unknown as HTMLElement;
    const container = {
      getBoundingClientRect: () => ({ top: 0, bottom: 600 }),
      querySelectorAll: () => [row(7, 360, 590), row(8, 640, 780), row(9, 780, 920)],
    } as unknown as HTMLElement;

    expect(shouldFollowChatSendFromViewport(container, 10)).toBe(true);
  });

  it("counts message blocks instead of pixels for a long visible message", () => {
    const lastVisibleMessageIndex = getLastVisibleMessageIndex({
      viewportTop: 0,
      viewportBottom: 600,
      rows: [{ index: 4, top: -1200, bottom: 580 }],
    });

    expect(lastVisibleMessageIndex).toBe(4);
    expect(shouldFollowChatSend({ messageCount: 7, lastVisibleMessageIndex })).toBe(true);
    expect(shouldFollowChatSend({ messageCount: 8, lastVisibleMessageIndex })).toBe(false);
  });

  it("fails closed when no rendered row is truly visible", () => {
    expect(shouldFollowChatSend({ messageCount: 4, lastVisibleMessageIndex: null })).toBe(false);
  });
});
