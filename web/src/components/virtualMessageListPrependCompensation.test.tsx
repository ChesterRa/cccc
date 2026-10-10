// @vitest-environment happy-dom

import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  usePrependCompensationController,
  useTopHistoryLoadCoordinator,
} from "./virtualMessageListPrependCompensation";

type Coordinator = ReturnType<typeof useTopHistoryLoadCoordinator>;

const ANCHOR_ID = "evt-anchor";
const SCROLL_PARAMS = { topTriggerPx: 80, topRearmPx: 240, hasMoreHistory: true };

/** What the reader currently sees: the anchor row's offset, viewport top, and whether it is rendered. */
const view = { offsetPx: 0, anchorTop: 0, anchorRendered: true };

function Probe({
  scrollToMessageAnchor,
  revealMessageAnchor,
  onAnchorVerified,
  onReady,
}: {
  scrollToMessageAnchor: (eventId: string, offsetPx?: number) => boolean;
  revealMessageAnchor?: (eventId: string) => void;
  onAnchorVerified?: () => void;
  onReady: (coordinator: Coordinator, scrollEl: HTMLDivElement | null) => void;
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
    onAnchorVerified,
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
    onLoadMore: () => {},
  });
  return (
    <div
      ref={(el) => {
        parentRef.current = el;
        onReady(coordinator, el);
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

  it("reports each check of the anchor against the DOM so held-back shifts can be dropped", async () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    const onAnchorVerified = vi.fn();
    let coordinator: Coordinator | null = null;
    let scrollEl: HTMLDivElement | null = null;
    await act(async () =>
      root.render(
        <Probe
          scrollToMessageAnchor={() => true}
          onAnchorVerified={onAnchorVerified}
          onReady={(next, el) => {
            coordinator = next;
            scrollEl = el;
          }}
        />,
      ),
    );

    view.offsetPx = -50;
    view.anchorTop = 167;
    coordinator!.handleTopHistoryScroll({
      ...SCROLL_PARAMS,
      scrollTop: 0,
      isLoadingHistory: false,
    });
    coordinator!.applyPendingPrependCompensation({ isLoadingHistory: false });
    scrollEl!.scrollTop = 15_568;

    // Not rendered yet: nothing was checked against the DOM.
    view.anchorRendered = false;
    vi.advanceTimersToNextFrame();
    expect(onAnchorVerified).not.toHaveBeenCalled();

    // Rendered off by 56px: corrected, and the check is reported.
    view.anchorRendered = true;
    view.anchorTop = 111;
    vi.advanceTimersToNextFrame();
    expect(scrollEl!.scrollTop).toBe(15_512);
    expect(onAnchorVerified).toHaveBeenCalledTimes(1);

    // In place on the following frames: still reported until the correction settles.
    view.anchorTop = 167;
    vi.advanceTimersToNextFrame();
    vi.advanceTimersToNextFrame();
    expect(onAnchorVerified).toHaveBeenCalledTimes(3);
  });
});
