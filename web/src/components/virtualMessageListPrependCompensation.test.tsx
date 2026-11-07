// @vitest-environment happy-dom

import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  usePrependCompensationController,
  useTopHistoryLoadCoordinator,
} from "./virtualMessageListPrependCompensation";

type Coordinator = ReturnType<typeof useTopHistoryLoadCoordinator>;
type Compensation = ReturnType<typeof usePrependCompensationController>;

const ANCHOR_ID = "evt-anchor";
const SCROLL_PARAMS = { topTriggerPx: 80, topRearmPx: 240, hasMoreHistory: true };

/** What the reader currently sees: the anchor row's offset, viewport top, and whether it is rendered. */
const view = { offsetPx: 0, anchorTop: 0, anchorRendered: true };

function Probe({
  scrollToMessageAnchor,
  revealMessageAnchor,
  onLoadMore = () => {},
  onReady,
}: {
  scrollToMessageAnchor: (eventId: string, offsetPx?: number) => boolean;
  revealMessageAnchor?: (eventId: string) => void;
  onLoadMore?: () => void | false | Promise<void | false>;
  onReady: (
    coordinator: Coordinator,
    scrollEl: HTMLDivElement | null,
    compensation: Compensation,
  ) => void;
}) {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const lastScrollTopRef = useRef(0);
  const compensation = usePrependCompensationController({
    parentRef,
    lastScrollTopRef,
    getMessageRowById: () => {
      if (!view.anchorRendered) return null;
      const row = document.createElement("div");
      row.getBoundingClientRect = () => new DOMRect(0, view.anchorTop);
      return row;
    },
    isVirtualized: false,
    scrollToVirtualOffset: () => {},
  });
  const coordinator = useTopHistoryLoadCoordinator({
    compensation,
    getAnchorSnapshot: () => ({ anchorId: ANCHOR_ID, offsetPx: view.offsetPx }),
    getAnchorTop: () => view.anchorTop,
    getCurrentContentSize: () => 2_000,
    scrollToMessageAnchor,
    revealMessageAnchor,
    cancelPendingBottomScroll: () => {},
    detachFollowMode: () => {},
    markAwayFromBottom: () => {},
    onLoadMore,
  });
  return (
    <div
      ref={(el) => {
        parentRef.current = el;
        onReady(coordinator, el, compensation);
      }}
    />
  );
}

describe("useTopHistoryLoadCoordinator", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    Object.assign(view, { offsetPx: 0, anchorTop: 0, anchorRendered: true });
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  });

  it("lets a manual request pass the top-scroll gate while keeping one anchor and one request", async () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    const loadMore = vi.fn();
    const scrollToMessageAnchor = vi.fn(() => true);
    let coordinator: Coordinator | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={scrollToMessageAnchor}
          onLoadMore={loadMore}
          onReady={(next) => {
            coordinator = next;
          }}
        />,
      ),
    );

    expect(
      coordinator!.requestTopHistoryLoad({
        scrollTop: 0,
        hasMoreHistory: false,
        isLoadingHistory: false,
      }),
    ).toBe(false);
    expect(
      coordinator!.requestTopHistoryLoad({
        scrollTop: 0,
        hasMoreHistory: true,
        isLoadingHistory: true,
      }),
    ).toBe(false);
    expect(loadMore).not.toHaveBeenCalled();

    view.offsetPx = 20;
    expect(
      coordinator!.handleTopHistoryScroll({
        ...SCROLL_PARAMS,
        scrollTop: 0,
        isLoadingHistory: false,
      }),
    ).toBe(true);
    // A button press before the loading state is rendered cannot start a second request.
    expect(
      coordinator!.requestTopHistoryLoad({
        scrollTop: 0,
        hasMoreHistory: true,
        isLoadingHistory: false,
      }),
    ).toBe(false);
    expect(loadMore).toHaveBeenCalledTimes(1);
    expect(coordinator!.applyPendingPrependCompensation({ isLoadingHistory: false })).toBe(true);
    expect(scrollToMessageAnchor).toHaveBeenCalledWith(ANCHOR_ID, 20);
    vi.advanceTimersToNextFrame();
    vi.advanceTimersToNextFrame();

    // The auto gate is still disarmed, but the explicit action can fetch another page.
    view.offsetPx = 35;
    expect(
      coordinator!.requestTopHistoryLoad({
        scrollTop: 0,
        hasMoreHistory: true,
        isLoadingHistory: false,
      }),
    ).toBe(true);
    expect(loadMore).toHaveBeenCalledTimes(2);
    expect(coordinator!.applyPendingPrependCompensation({ isLoadingHistory: false })).toBe(true);
    expect(scrollToMessageAnchor).toHaveBeenLastCalledWith(ANCHOR_ID, 35);
  });

  it("releases its gate when the loader says it did not start", async () => {
    const onLoadMore = vi
      .fn()
      .mockReturnValueOnce(false)
      .mockResolvedValueOnce(false)
      .mockReturnValue(new Promise<void>(() => {}));
    let coordinator: Coordinator | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => true}
          onLoadMore={onLoadMore}
          onReady={(next) => (coordinator = next)}
        />,
      ),
    );
    const params = { scrollTop: 0, hasMoreHistory: true, isLoadingHistory: false };
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(false);
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(true);
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(false);
    await act(async () => Promise.resolve());
    expect(
      coordinator!.handleTopHistoryScroll({
        ...SCROLL_PARAMS,
        scrollTop: 0,
        isLoadingHistory: false,
      }),
    ).toBe(true);
    expect(onLoadMore).toHaveBeenCalledTimes(3);
  });

  it("does not let a late no-op clear a newer request", async () => {
    let resolveFirst!: (result: false) => void;
    const first = new Promise<false>((resolve) => (resolveFirst = resolve));
    const onLoadMore = vi
      .fn()
      .mockReturnValueOnce(first)
      .mockReturnValue(new Promise<void>(() => {}));
    let coordinator: Coordinator | null = null;
    let compensation: Compensation | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => true}
          onLoadMore={onLoadMore}
          onReady={(next, _el, controller) => {
            coordinator = next;
            compensation = controller;
          }}
        />,
      ),
    );
    const params = { scrollTop: 0, hasMoreHistory: true, isLoadingHistory: false };
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(true);
    coordinator!.reset();
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(true);
    await act(async () => resolveFirst(false));
    expect(compensation!.pendingRef.current).not.toBeNull();
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(false);
    expect(onLoadMore).toHaveBeenCalledTimes(2);
  });

  it("releases its gate if the loader rejects before reporting a result", async () => {
    const logError = vi.spyOn(console, "error").mockImplementation(() => {});
    const onLoadMore = vi
      .fn()
      .mockRejectedValueOnce(new Error("fixture"))
      .mockReturnValue(new Promise<void>(() => {}));
    let coordinator: Coordinator | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => true}
          onLoadMore={onLoadMore}
          onReady={(next) => (coordinator = next)}
        />,
      ),
    );
    const params = { scrollTop: 0, hasMoreHistory: true, isLoadingHistory: false };
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(true);
    await act(async () => Promise.resolve());
    expect(coordinator!.requestTopHistoryLoad(params)).toBe(true);
    expect(onLoadMore).toHaveBeenCalledTimes(2);
    expect(logError).toHaveBeenCalledOnce();
    logError.mockRestore();
  });

  it("keeps an appended page's offset when its anchor cannot be rendered", async () => {
    let coordinator: Coordinator | null = null;
    let compensation: Compensation | null = null;
    let scrollEl: HTMLDivElement | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => false}
          onReady={(next, el, controller) => {
            coordinator = next;
            scrollEl = el;
            compensation = controller;
          }}
        />,
      ),
    );
    const scrollTo = vi.spyOn(scrollEl!, "scrollTo");
    compensation!.begin({
      kind: "append",
      previousOffset: 123,
      previousTotalSize: 1000,
      anchorId: "",
      anchorOffsetPx: 0,
      anchorTop: null,
    });
    expect(coordinator!.applyPendingPrependCompensation({ isLoadingHistory: false })).toBe(true);
    expect(scrollTo).toHaveBeenCalledWith({ top: 123, behavior: "auto" });
  });

  it("keeps append semantics when the reader moves during the request", async () => {
    let coordinator: Coordinator | null = null;
    let compensation: Compensation | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => true}
          onReady={(next, _el, controller) => {
            coordinator = next;
            compensation = controller;
          }}
        />,
      ),
    );
    compensation!.begin({
      kind: "append",
      previousOffset: 123,
      previousTotalSize: 1000,
      anchorId: ANCHOR_ID,
      anchorOffsetPx: 0,
      anchorTop: 0,
    });
    coordinator!.handleTopHistoryScroll({
      ...SCROLL_PARAMS,
      scrollTop: 250,
      isLoadingHistory: true,
    });
    expect(compensation!.pendingRef.current).toMatchObject({ kind: "append", previousOffset: 250 });
  });

  it("restores the position the reader scrolled to while history was loading", async () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    const scrollToMessageAnchor = vi.fn(() => true);
    let coordinator: Coordinator | null = null;
    let scrollEl: HTMLDivElement | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={scrollToMessageAnchor}
          onReady={(next, el) => {
            coordinator = next;
            scrollEl = el;
          }}
        />,
      ),
    );

    // Crossing the trigger line starts the load.
    view.offsetPx = 30;
    view.anchorTop = 70;
    expect(
      coordinator!.handleTopHistoryScroll({
        ...SCROLL_PARAMS,
        scrollTop: 70,
        isLoadingHistory: false,
      }),
    ).toBe(true);

    // Momentum keeps scrolling up to the top before the older page arrives.
    view.offsetPx = -40;
    view.anchorTop = 140;
    coordinator!.handleTopHistoryScroll({ ...SCROLL_PARAMS, scrollTop: 0, isLoadingHistory: true });

    // The page is prepended; the anchor must land where the reader left it.
    expect(coordinator!.applyPendingPrependCompensation({ isLoadingHistory: false })).toBe(true);
    expect(scrollToMessageAnchor).toHaveBeenCalledWith(ANCHOR_ID, -40);

    // The follow-up frame correction must not pull the row back to the trigger-time spot.
    scrollEl!.scrollTop = 900;
    vi.advanceTimersToNextFrame();
    vi.advanceTimersToNextFrame();
    expect(scrollEl!.scrollTop).toBe(900);
  });

  it("keeps the anchor correction alive until a virtual anchor row renders again", async () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    let coordinator: Coordinator | null = null;
    let scrollEl: HTMLDivElement | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => true}
          onReady={(next, el) => {
            coordinator = next;
            scrollEl = el;
          }}
        />,
      ),
    );

    view.offsetPx = -88;
    view.anchorTop = 189;
    coordinator!.handleTopHistoryScroll({
      ...SCROLL_PARAMS,
      scrollTop: 0,
      isLoadingHistory: false,
    });
    coordinator!.applyPendingPrependCompensation({ isLoadingHistory: false });
    scrollEl!.scrollTop = 48_117;

    // Real heights of rows above the anchor are still being measured, so the
    // anchor is briefly outside the rendered range.
    view.anchorRendered = false;
    vi.advanceTimersToNextFrame();
    vi.advanceTimersToNextFrame();

    // The virtualizer brings it back 16px lower than where the reader left it.
    view.anchorRendered = true;
    view.anchorTop = 205;
    vi.advanceTimersToNextFrame();
    expect(scrollEl!.scrollTop).toBe(48_133);
  });

  it("hands a still-missing virtual anchor to the reveal once, then corrects by its real top", async () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    const revealMessageAnchor = vi.fn();
    let coordinator: Coordinator | null = null;
    let scrollEl: HTMLDivElement | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => true}
          revealMessageAnchor={revealMessageAnchor}
          onReady={(next, el) => {
            coordinator = next;
            scrollEl = el;
          }}
        />,
      ),
    );

    view.offsetPx = -56;
    view.anchorTop = 173;
    coordinator!.handleTopHistoryScroll({
      ...SCROLL_PARAMS,
      scrollTop: 0,
      isLoadingHistory: false,
    });
    coordinator!.applyPendingPrependCompensation({ isLoadingHistory: false });

    // Estimates were so far off that the virtualizer does not bring the anchor back.
    view.anchorRendered = false;
    vi.advanceTimersToNextFrame();
    expect(revealMessageAnchor).not.toHaveBeenCalled();
    vi.advanceTimersToNextFrame();
    vi.advanceTimersToNextFrame();
    expect(revealMessageAnchor).toHaveBeenCalledTimes(1);
    expect(revealMessageAnchor).toHaveBeenCalledWith(ANCHOR_ID);

    // The reveal puts the row at the list top; restore the reader's offset.
    scrollEl!.scrollTop = 19_226;
    view.anchorRendered = true;
    view.anchorTop = 117;
    vi.advanceTimersToNextFrame();
    expect(scrollEl!.scrollTop).toBe(19_170);
  });
});
