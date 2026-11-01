// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../types";
import type { VirtualMessageListProps } from "./virtualMessageList/types";
import { VirtualMessageList } from "./VirtualMessageList";

// The opening place, bottom follow and anchor restoration run for real here; only rendering
// and unrelated follow hooks are stubbed.
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: () => ({
    getVirtualItems: () => [],
    getTotalSize: () => 0,
    measureElement: () => {},
    scrollToIndex: () => {},
    scrollToOffset: () => {},
  }),
}));
vi.mock("./virtualMessageList/useVirtualMessageMetadata", () => ({
  useVirtualMessageMetadata: () => ({
    messageTextById: new Map(),
    agentStateById: new Map(),
    actorById: new Map(),
    displayNameMap: new Map(),
  }),
}));
vi.mock("./virtualMessageList/MessageRows", () => ({
  MessageRows: ({
    messages,
    contentRef,
    effectiveHighlightEventId,
  }: {
    messages: LedgerEvent[];
    contentRef: React.Ref<HTMLDivElement>;
    effectiveHighlightEventId?: string;
  }) => (
    <div ref={contentRef}>
      {messages.map((message) => (
        <div
          key={String(message.id)}
          data-message-row="true"
          data-message-id={String(message.id)}
          data-highlighted={effectiveHighlightEventId === message.id || undefined}
        />
      ))}
    </div>
  ),
}));
vi.mock("./virtualMessageList/useBottomScrollController", () => ({
  useBottomScrollController: () => ({
    activeRequestRef: { current: null },
    scrollToBottom: () => {},
  }),
}));
vi.mock("./virtualMessageList/useMessageTailAutoFollow", () => ({
  useMessageTailAutoFollow: () => {},
}));
vi.mock("./virtualMessageList/useViewChangeAutoFollow", () => ({
  useViewChangeAutoFollow: () => {},
}));
vi.mock("./virtualMessageList/useSendScrollRequestLifecycle", () => ({
  useSendScrollRequestLifecycle: () => {},
}));
vi.mock("./virtualMessageList/useReplyTargetNavigation", () => ({
  useReplyTargetNavigation: () => ({
    replyJumpHighlightId: "",
    replyJumpNotice: "",
    openReplyTarget: () => {},
  }),
}));

class FakeResizeObserver {
  observe() {}
  disconnect() {}
}
let host: HTMLDivElement;
let root: Root;
const frames = new Map<number, FrameRequestCallback>();
let nextFrame = 1;
const restores: Array<() => void> = [];

function flushFrames() {
  const due = [...frames.values()];
  frames.clear();
  for (const frame of due) frame(performance.now());
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  const request = vi.spyOn(window, "requestAnimationFrame").mockImplementation((frame) => {
    frames.set(nextFrame, frame);
    return nextFrame++;
  });
  const cancel = vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    frames.delete(id);
  });
  // A list taller than its view, read at the top: away from the bottom.
  const scroll = vi.spyOn(Element.prototype, "scrollHeight", "get").mockReturnValue(1000);
  const client = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
  restores.push(...[request, cancel, scroll, client].map((spy) => () => spy.mockRestore()));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  frames.clear();
  for (const restore of restores.splice(0)) restore();
  vi.unstubAllGlobals();
});

const props = (): VirtualMessageListProps => ({
  messages: ["m1", "m2"].map((id) => ({
    id,
    kind: "chat.message",
    by: "agent",
    data: { text: id },
  })),
  actors: [],
  agentStates: [],
  taskById: new Map(),
  isDark: false,
  groupId: "g1",
  groupLabelById: {},
  viewKey: "g1:live:all:[]",
  initialScrollAnchorId: "m1",
  initialScrollAnchorOffsetPx: 40,
  highlightInitialScrollAnchor: true,
  onReply: vi.fn(),
  onShowRecipients: vi.fn(),
  showScrollButton: false,
  onScrollButtonClick: vi.fn(),
  chatUnreadCount: 0,
});
const anchorMarked = () =>
  host.querySelector('[data-message-id="m1"]')!.hasAttribute("data-highlighted");

it("opens on its place although the list scrolls itself before that frame", async () => {
  await act(async () => root.render(<VirtualMessageList {...props()} />));
  // As when the virtualizer applies its initial offset: a scroll away from the bottom cancels
  // pending bottom scrolls, which must not take the opening place with them.
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  await act(async () => log.dispatchEvent(new Event("scroll")));
  await act(async () => flushFrames());
  expect(anchorMarked()).toBe(true);
});

it("leaves the place to a reader who acts before that frame", async () => {
  await act(async () => root.render(<VirtualMessageList {...props()} />));
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  await act(async () => log.dispatchEvent(new Event("wheel")));
  await act(async () => flushFrames());
  expect(anchorMarked()).toBe(false);
});
