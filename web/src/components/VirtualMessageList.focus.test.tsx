// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { VirtualMessageListProps } from "./virtualMessageList/types";
import { VirtualMessageList } from "./VirtualMessageList";

const mocks = vi.hoisted(() => {
  const state = { forced: true };
  return {
    state,
    cancel: vi.fn(() => {
      state.forced = false;
    }),
    scroll: vi.fn(),
    shouldForce: () => state.forced,
    noop: () => {},
    active: { current: null },
  };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { displayCount?: number | string }) =>
      key === "scrollToBottom"
        ? "Back to latest"
        : key === "scrollToBottomUnread"
          ? `${options?.displayCount} new messages`
          : key,
  }),
}));
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
  MessageRows: ({ contentRef }: { contentRef: React.Ref<HTMLDivElement> }) => (
    <div ref={contentRef}>
      <button type="button" data-message-focus>
        Message action
      </button>
    </div>
  ),
}));
vi.mock("./virtualMessageList/useForcedBottomFollow", () => ({
  useForcedBottomFollow: () => ({
    cancelPendingBottomScroll: mocks.cancel,
    cancelScheduledScroll: mocks.noop,
    scheduleForceStickToBottom: mocks.noop,
    shouldForceStickToBottom: mocks.shouldForce,
  }),
  useForcedBottomFollowKeyboardCancel: () => mocks.noop,
}));
vi.mock("./virtualMessageList/useBottomScrollController", () => ({
  useBottomScrollController: () => ({
    activeRequestRef: mocks.active,
    scrollToBottom: mocks.scroll,
  }),
}));
vi.mock("./virtualMessageList/useInitialMessageScroll", () => ({
  useInitialMessageScroll: () => {},
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
const props = (): VirtualMessageListProps => ({
  messages: [{ id: "m1", kind: "chat.message", by: "agent", data: { text: "message" } }],
  actors: [],
  agentStates: [],
  taskById: new Map(),
  isDark: false,
  groupId: "g1",
  groupLabelById: {},
  onReply: vi.fn(),
  onShowRecipients: vi.fn(),
  showScrollButton: true,
  onScrollButtonClick: vi.fn(),
  chatUnreadCount: 0,
});

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  mocks.state.forced = true;
  mocks.cancel.mockClear();
  mocks.scroll.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

it("cancels forced bottom follow when keyboard focus enters a message", async () => {
  await act(async () => root.render(<VirtualMessageList {...props()} />));
  mocks.cancel.mockClear();
  mocks.state.forced = true;
  await act(async () => host.querySelector<HTMLButtonElement>("[data-message-focus]")!.focus());
  expect(mocks.cancel).toHaveBeenCalledOnce();
  expect(mocks.state.forced).toBe(false);
});

it("keeps the latest action outside the log and reports capped unread messages", async () => {
  const input = props();
  await act(async () => root.render(<VirtualMessageList {...input} />));
  expect(host.querySelector("[data-scroll-latest]")?.textContent).toContain("Back to latest");
  expect(host.querySelector("[data-scroll-latest-count]")).toBeNull();

  input.chatUnreadCount = 101;
  await act(async () => root.render(<VirtualMessageList {...input} />));
  const log = host.querySelector('[role="log"]')!;
  const button = host.querySelector<HTMLButtonElement>("[data-scroll-latest]")!;
  expect(log.contains(button)).toBe(false);
  expect(button.textContent).toContain("Back to latest");
  expect(button.querySelector("[data-scroll-latest-count]")?.textContent).toBe("99+");
  expect(button.getAttribute("aria-label")).toBe("Back to latest · 99+ new messages");
  await act(async () => button.click());
  expect(mocks.scroll).toHaveBeenCalledWith({ force: true });
  expect(input.onScrollButtonClick).toHaveBeenCalledOnce();
  await act(async () => root.render(<VirtualMessageList {...input} showScrollButton={false} />));
  expect(host.querySelector("[data-scroll-latest]")).toBeNull();
  await act(async () => root.render(<VirtualMessageList {...input} readOnly />));
  expect(host.querySelector("[data-scroll-latest]")).toBeNull();
});
