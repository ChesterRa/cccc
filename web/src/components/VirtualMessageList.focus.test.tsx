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
  for (const restore of heightRestores.splice(0)) restore();
});

// This page lays nothing out, so every list looks too short to scroll unless given a height.
const heightRestores: Array<() => void> = [];
function logHeights(scrollHeight: number, clientHeight = 400) {
  const scroll = vi.spyOn(Element.prototype, "scrollHeight", "get").mockReturnValue(scrollHeight);
  const client = vi
    .spyOn(HTMLElement.prototype, "clientHeight", "get")
    .mockReturnValue(clientHeight);
  heightRestores.push(
    () => scroll.mockRestore(),
    () => client.mockRestore(),
  );
}

it("cancels forced bottom follow when keyboard focus enters a message", async () => {
  await act(async () => root.render(<VirtualMessageList {...props()} />));
  mocks.cancel.mockClear();
  mocks.state.forced = true;
  await act(async () => host.querySelector<HTMLButtonElement>("[data-message-focus]")!.focus());
  expect(mocks.cancel).toHaveBeenCalledOnce();
  expect(mocks.state.forced).toBe(false);
});

it("keeps the latest action outside the log and reports capped unread messages", async () => {
  logHeights(1000);
  const input = props();
  await act(async () => root.render(<VirtualMessageList {...input} />));
  expect(host.querySelector("[data-scroll-latest]")?.getAttribute("aria-label")).toBe(
    "Back to latest",
  );
  expect(host.querySelector("[data-scroll-latest-count]")).toBeNull();

  input.chatUnreadCount = 101;
  await act(async () => root.render(<VirtualMessageList {...input} />));
  const log = host.querySelector('[role="log"]')!;
  const button = host.querySelector<HTMLButtonElement>("[data-scroll-latest]")!;
  expect(log.contains(button)).toBe(false);
  expect(button.textContent).toBe("99+");
  expect(button.querySelector("[data-scroll-latest-count]")?.textContent).toBe("99+");
  expect(button.getAttribute("aria-label")).toBe("Back to latest · 99+ new messages");
  expect(button.title).toBe("Back to latest · 99+ new messages");
  await act(async () => button.click());
  expect(mocks.scroll).toHaveBeenCalledWith({ force: true });
  expect(input.onScrollButtonClick).toHaveBeenCalledOnce();
  await act(async () => root.render(<VirtualMessageList {...input} showScrollButton={false} />));
  expect(host.querySelector("[data-scroll-latest]")).toBeNull();
  await act(async () => root.render(<VirtualMessageList {...input} readOnly />));
  expect(host.querySelector("[data-scroll-latest]")).toBeNull();
});

it("hands a context's latest button to its owner instead of loading a page", async () => {
  logHeights(1000);
  const input = props();
  const onReachedContextEnd = vi.fn();
  await act(async () =>
    root.render(
      <VirtualMessageList {...input} contextWindow onReachedContextEnd={onReachedContextEnd} />,
    ),
  );
  mocks.scroll.mockClear();
  const button = host.querySelector<HTMLButtonElement>("[data-scroll-latest]")!;
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  await act(async () => button.focus());
  await act(async () =>
    button.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 })),
  );
  expect(document.activeElement).toBe(log);
  expect(input.onScrollButtonClick).toHaveBeenCalledOnce();
  expect(mocks.scroll).not.toHaveBeenCalled();
  expect(onReachedContextEnd).not.toHaveBeenCalled();
});

it("saves no place for a view that was only looked at, and saves its view once acted in", async () => {
  const onScrollSnapshot = vi.fn();
  await act(async () =>
    root.render(
      <VirtualMessageList
        {...props()}
        viewKey="g1:live:mail"
        onScrollSnapshot={onScrollSnapshot}
      />,
    ),
  );
  await act(async () => root.render(<div />));
  expect(onScrollSnapshot).not.toHaveBeenCalled();

  await act(async () =>
    root.render(
      <VirtualMessageList
        {...props()}
        viewKey="g1:live:mail"
        onScrollSnapshot={onScrollSnapshot}
      />,
    ),
  );
  host.querySelector('[role="log"]')!.dispatchEvent(new Event("wheel"));
  await act(async () => root.render(<div />));
  expect(onScrollSnapshot).toHaveBeenCalledOnce();
  expect(onScrollSnapshot.mock.calls[0][0]).toMatchObject({ viewKey: "g1:live:mail" });
  expect(onScrollSnapshot.mock.calls[0][1]).toBe("g1");
});

it("tells the page when a view opens away from the latest messages, and only then", async () => {
  // The restore hook is mocked out here, as a scroll event can cancel it in a browser.
  const onScrollChange = vi.fn();
  await act(async () =>
    root.render(
      <VirtualMessageList
        {...props()}
        viewKey="g1:live:all"
        initialScrollAnchorId="m1"
        onScrollChange={onScrollChange}
      />,
    ),
  );
  expect(onScrollChange).toHaveBeenCalledExactlyOnceWith(false);

  // A view without a place yet, such as one whose history is still loading, must not report
  // the bottom: that would clear the unread count before its place arrives.
  await act(async () =>
    root.render(
      <VirtualMessageList
        {...props()}
        viewKey={'g1:live:all:["reviewer"]'}
        onScrollChange={onScrollChange}
      />,
    ),
  );
  expect(onScrollChange).toHaveBeenCalledOnce();
});

it("keeps the key to the latest out of a list too short to scroll until it has work", async () => {
  logHeights(400);
  const input = props();
  await act(async () => root.render(<VirtualMessageList {...input} />));
  expect(host.querySelector("[data-scroll-latest]")).toBeNull();

  // A message this view filters out is counted, and the key shows the count.
  await act(async () => root.render(<VirtualMessageList {...input} chatUnreadCount={2} />));
  expect(host.querySelector("[data-scroll-latest-count]")?.textContent).toBe("2");

  // Once the list scrolls, it has somewhere to go.
  await act(async () => root.render(<VirtualMessageList {...input} />));
  expect(host.querySelector("[data-scroll-latest]")).toBeNull();
  await act(async () => host.querySelector('[role="log"]')!.dispatchEvent(new Event("scroll")));
  expect(host.querySelector("[data-scroll-latest]")).not.toBeNull();
});

it("returns keyboard focus to the log before forcing the latest position", async () => {
  logHeights(1000);
  const input = props();
  await act(async () => root.render(<VirtualMessageList {...input} />));
  const button = host.querySelector<HTMLButtonElement>("[data-scroll-latest]")!;
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  await act(async () => button.focus());
  mocks.cancel.mockClear();
  mocks.scroll.mockClear();
  mocks.state.forced = true;

  await act(async () => button.click());
  expect(document.activeElement).toBe(log);
  expect(mocks.cancel).toHaveBeenCalled();
  expect(mocks.scroll).toHaveBeenCalledWith({ force: true });
  expect(mocks.cancel.mock.invocationCallOrder[0]).toBeLessThan(
    mocks.scroll.mock.invocationCallOrder[0],
  );

  await act(async () => root.render(<VirtualMessageList {...input} showScrollButton={false} />));
  expect(document.activeElement).toBe(log);
});

it("uses the delayed pixel spinner while history loads", async () => {
  vi.useFakeTimers();
  try {
    await act(async () => root.render(<VirtualMessageList {...props()} isLoadingHistory />));
    expect(host.querySelector(".lcd-spinner")?.hasAttribute("data-shown")).toBe(false);
    expect(host.querySelector(".animate-spin")).toBeNull();
    await act(async () => vi.advanceTimersByTime(250));
    expect(host.querySelector(".lcd-spinner")?.hasAttribute("data-shown")).toBe(true);
  } finally {
    vi.useRealTimers();
  }
});
