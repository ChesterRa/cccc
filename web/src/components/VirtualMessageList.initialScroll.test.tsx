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
const virtualizerCalls = vi.hoisted(() => [] as boolean[]);
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: (options: { enabled: boolean }) => {
    virtualizerCalls.push(options.enabled);
    return {
      getVirtualItems: () => [],
      getTotalSize: () => 0,
      measureElement: () => {},
      scrollToIndex: () => {},
      scrollToOffset: () => {},
    };
  },
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
  virtualizerCalls.length = 0;
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

it("does not move a reader from a context after they have begun scrolling", async () => {
  const onContextReady = vi.fn();
  const values = {
    ...props(),
    initialScrollTargetId: "m1",
    initialScrollAnchorId: undefined,
    onContextReady,
  };
  await act(async () => root.render(<VirtualMessageList {...values} />));
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
  const target = host.querySelector<HTMLElement>('[data-message-id="m1"]')!;
  target.getBoundingClientRect = () => ({ top: 10, bottom: 60 }) as DOMRect;
  await act(async () => log.dispatchEvent(new Event("wheel")));
  await act(async () => {
    flushFrames();
    flushFrames();
    flushFrames();
  });
  expect(onContextReady).not.toHaveBeenCalled();
});

it("only treats a reader's downward scroll as reaching a context's end", async () => {
  const onReachedContextEnd = vi.fn();
  const values = {
    ...props(),
    initialScrollTargetId: "m1",
    initialScrollAnchorId: undefined,
    onReachedContextEnd,
  };
  await act(async () => root.render(<VirtualMessageList {...values} />));
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
  const last = host.querySelector<HTMLElement>('[data-message-id="m2"]')!;
  last.getBoundingClientRect = () =>
    ({ top: 900 - log.scrollTop, bottom: 950 - log.scrollTop }) as DOMRect;
  await act(async () => {
    flushFrames();
    flushFrames();
    flushFrames();
  });
  // A programmatic scroll must not leave the window.
  log.scrollTop = 600;
  await act(async () => log.dispatchEvent(new Event("scroll")));
  await act(async () => flushFrames());
  expect(onReachedContextEnd).not.toHaveBeenCalled();
  log.scrollTop = 400;
  await act(async () => log.dispatchEvent(new Event("scroll")));
  await act(async () => flushFrames());
  // A real scroll intent followed by moving down to the end may leave it.
  await act(async () => log.dispatchEvent(new Event("wheel")));
  log.scrollTop = 600;
  await act(async () => log.dispatchEvent(new Event("scroll")));
  expect(onReachedContextEnd).toHaveBeenCalledWith(false, true);
});

it("starts following after a live target opens at the bottom without a scroll event", async () => {
  const onScrollChange = vi.fn();
  vi.spyOn(Element.prototype, "scrollHeight", "get").mockReturnValue(400);
  const values = { ...props(), liveJumpActive: true, onScrollChange };
  await act(async () => root.render(<VirtualMessageList {...values} />));
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
  const last = host.querySelector<HTMLElement>('[data-message-id="m2"]')!;
  last.getBoundingClientRect = () => ({ top: 300, bottom: 350 }) as DOMRect;
  await act(async () => {
    flushFrames();
    flushFrames();
    flushFrames();
  });
  expect(onScrollChange).toHaveBeenCalledWith(true);
});

it("loads at most one newer page for one smooth downward scroll", async () => {
  const onReachedContextEnd = vi.fn(() => true);
  const values = {
    ...props(),
    initialScrollTargetId: "m1",
    initialScrollAnchorId: undefined,
    onReachedContextEnd,
  };
  await act(async () => root.render(<VirtualMessageList {...values} />));
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
  const last = host.querySelector<HTMLElement>('[data-message-id="m2"]')!;
  last.getBoundingClientRect = () =>
    ({ top: 900 - log.scrollTop, bottom: 950 - log.scrollTop }) as DOMRect;
  await act(async () => {
    flushFrames();
    flushFrames();
    flushFrames();
  });
  log.scrollTop = 400;
  await act(async () => log.dispatchEvent(new Event("wheel")));
  log.scrollTop = 500;
  await act(async () => log.dispatchEvent(new Event("scroll")));
  log.scrollTop = 600;
  await act(async () => {
    log.dispatchEvent(new Event("scroll"));
    flushFrames();
    flushFrames();
  });
  expect(onReachedContextEnd).toHaveBeenCalledTimes(1);
});

it.each(["wheel", "End"])(
  "continues a context on %s at the bottom without a scroll event",
  async (gesture) => {
    const onReachedContextEnd = vi.fn(() => true);
    const values = {
      ...props(),
      initialScrollTargetId: "m1",
      initialScrollAnchorId: undefined,
      onReachedContextEnd,
    };
    await act(async () => root.render(<VirtualMessageList {...values} />));
    const log = host.querySelector<HTMLElement>('[role="log"]')!;
    log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
    const last = host.querySelector<HTMLElement>('[data-message-id="m2"]')!;
    last.getBoundingClientRect = () => ({ top: 300, bottom: 350 }) as DOMRect;
    await act(async () => {
      flushFrames();
      flushFrames();
    });
    log.scrollTop = 600;
    await act(async () => {
      if (gesture === "wheel")
        log.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: 40 }));
      else log.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "End" }));
    });
    expect(onReachedContextEnd).toHaveBeenCalledWith(false, true);
  },
);

it("does not continue the context for a wheel consumed by a nested scroller", async () => {
  const onReachedContextEnd = vi.fn(() => true);
  await act(async () =>
    root.render(<VirtualMessageList {...props()} onReachedContextEnd={onReachedContextEnd} />),
  );
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  const inner = document.createElement("div");
  inner.style.overflowY = "auto";
  Object.defineProperty(inner, "scrollHeight", { value: 1000 });
  Object.defineProperty(inner, "clientHeight", { value: 200 });
  log.append(inner);
  log.scrollTop = 600;
  await act(async () =>
    inner.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: 40 })),
  );
  expect(onReachedContextEnd).not.toHaveBeenCalled();
});

it.each([false, true])(
  "only completes a delayed live handoff if the reader has not moved away (moved=%s)",
  async (movedAway) => {
    const beforeTail = vi.fn(() => false);
    const afterTail = vi.fn(() => false);
    const values = {
      ...props(),
      contextWindow: true,
      initialScrollTargetId: "m1",
      initialScrollAnchorId: undefined,
    };
    await act(async () =>
      root.render(<VirtualMessageList {...values} onReachedContextEnd={beforeTail} />),
    );
    const log = host.querySelector<HTMLElement>('[role="log"]')!;
    log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
    const last = host.querySelector<HTMLElement>('[data-message-id="m2"]')!;
    last.getBoundingClientRect = () => ({ top: 300, bottom: 350 }) as DOMRect;
    await act(async () => {
      flushFrames();
      flushFrames();
    });
    log.scrollTop = 600;
    await act(async () =>
      log.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: 40 })),
    );
    expect(beforeTail).toHaveBeenCalledWith(false, true);
    if (movedAway)
      await act(async () =>
        log.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: -40 })),
      );
    await act(async () =>
      root.render(<VirtualMessageList {...values} onReachedContextEnd={afterTail} />),
    );
    if (movedAway) expect(afterTail).not.toHaveBeenCalled();
    else expect(afterTail).toHaveBeenCalledWith(false, true);
  },
);

it.each(["wheel", "End", "touch"])(
  "continues a short context on %s without a scroll event",
  async (gesture) => {
    const onReachedContextEnd = vi.fn(() => true);
    vi.spyOn(Element.prototype, "scrollHeight", "get").mockReturnValue(400);
    const values = {
      ...props(),
      contextWindow: true,
      initialScrollTargetId: "m1",
      initialScrollAnchorId: undefined,
      onReachedContextEnd,
    };
    await act(async () => root.render(<VirtualMessageList {...values} />));
    const log = host.querySelector<HTMLElement>('[role="log"]')!;
    log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
    for (const [id, top] of [
      ["m1", 100],
      ["m2", 300],
    ] as const) {
      host.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!.getBoundingClientRect = () =>
        ({ top, bottom: top + 50 }) as DOMRect;
    }
    await act(async () => {
      flushFrames();
      flushFrames();
    });
    onReachedContextEnd.mockClear();
    await act(async () => {
      if (gesture === "wheel")
        log.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: 40 }));
      else if (gesture === "End")
        log.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "End" }));
      else {
        for (const [type, y] of [
          ["touchstart", 200],
          ["touchmove", 150],
        ] as const) {
          const event = new Event(type, { bubbles: true });
          Object.defineProperty(event, "touches", { value: [{ clientY: y }] });
          log.dispatchEvent(event);
        }
      }
    });
    expect(onReachedContextEnd).toHaveBeenCalledWith(false, true);
  },
);

it("keeps a detached reader's place when the live list becomes virtualized", async () => {
  const messages = Array.from({ length: 79 }, (_, index) => ({
    id: `m${index}`,
    kind: "chat.message",
    by: "agent",
    data: { text: `message ${index}` },
  }));
  const values = { ...props(), messages, initialScrollAnchorId: undefined };
  await act(async () => root.render(<VirtualMessageList {...values} />));
  const log = host.querySelector<HTMLElement>('[role="log"]')!;
  log.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect;
  let rowTop = 100;
  const anchor = host.querySelector<HTMLElement>('[data-message-id="m1"]')!;
  anchor.getBoundingClientRect = () =>
    ({
      top: rowTop - (log.scrollTop - 400),
      bottom: rowTop - (log.scrollTop - 400) + 50,
    }) as DOMRect;
  await act(async () => {
    flushFrames();
    flushFrames();
  });
  log.scrollTop = 400;
  await act(async () => {
    log.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: -20 }));
    log.dispatchEvent(new Event("scroll"));
    flushFrames();
  });
  rowTop = 160;
  await act(async () =>
    root.render(
      <VirtualMessageList
        {...values}
        messages={[
          ...messages,
          { id: "m79", kind: "chat.message", by: "agent", data: { text: "new" } },
        ]}
      />,
    ),
  );
  await act(async () => {
    flushFrames();
    flushFrames();
  });
  expect(log.scrollTop).toBe(460);
});
