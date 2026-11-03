// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { MessageRows } from "./MessageRows";

vi.mock("../MessageBubble", () => ({ MessageBubble: () => null }));

const message = (id: string): LedgerEvent => ({ id, kind: "chat.message", by: "user" });

function props(messages: LedgerEvent[], shouldVirtualize: boolean) {
  return {
    messages,
    shouldVirtualize,
    virtualItems: messages.map((item, index) => ({
      key: String(item.id),
      index,
      start: index * 40,
    })),
    totalVirtualSize: messages.length * 40,
    nonVirtualTopMargin: 0,
    contentRef: { current: null },
    messageTextById: new Map(),
    actorById: new Map(),
    actors: [],
    agentStateById: new Map(),
    displayNameMap: new Map(),
    taskById: new Map(),
    isDark: false,
    groupId: "g",
    groupLabelById: {},
    onReply: vi.fn(),
    onShowRecipients: vi.fn(),
    onOpenReplyTarget: vi.fn(),
    measureElement: () => {},
  } satisfies ComponentProps<typeof MessageRows>;
}

it.each([false, true])(
  "marks only the last message, which keeps room under it for its actions (virtualized: %s)",
  async (shouldVirtualize) => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const host = document.createElement("div");
    const root = createRoot(host);
    const marks = () =>
      [...host.querySelectorAll<HTMLElement>("[data-message-row]")].map(
        (row) => row.dataset.lastMessage,
      );
    try {
      const messages = ["a", "b", "c"].map(message);
      await act(async () => root.render(<MessageRows {...props(messages, shouldVirtualize)} />));
      expect(marks()).toEqual([undefined, undefined, "true"]);
      // A new message takes the room; the one before it gives it back.
      const more = [...messages, message("d")];
      await act(async () => root.render(<MessageRows {...props(more, shouldVirtualize)} />));
      expect(marks()).toEqual([undefined, undefined, undefined, "true"]);
    } finally {
      await act(async () => root.unmount());
    }
  },
);
