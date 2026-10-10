// @vitest-environment happy-dom

import { Virtualizer, type VirtualizerOptions } from "@tanstack/react-virtual";
import { beforeAll, describe, expect, it } from "vite-plus/test";

import { discardDeferredScrollAdjustment, shouldAdjustScrollForResizedRow } from "./virtualOffset";

// The virtualizer caches its iOS detection per module, so this file runs as an
// iPhone from the start: row shifts are held back while the list is scrolling.
beforeAll(() => {
  Object.defineProperty(window.navigator, "userAgent", {
    configurable: true,
    value: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15",
  });
});

const ROW_PX = 100;
const VIEWPORT_PX = 600;
const READER_OFFSET = 2_000;

function createScrollingVirtualizer() {
  const scrollElement = document.createElement("div");
  Object.defineProperty(scrollElement, "scrollHeight", { configurable: true, value: 100_000 });
  Object.defineProperty(scrollElement, "clientHeight", { configurable: true, value: VIEWPORT_PX });
  const scrollWrites: Array<{ offset: number; adjustments?: number }> = [];
  let reportOffset: (offset: number, isScrolling: boolean) => void = () => {};
  const options: VirtualizerOptions<HTMLDivElement, Element> = {
    count: 50,
    getScrollElement: () => scrollElement,
    estimateSize: () => ROW_PX,
    scrollToFn: (offset, { adjustments }) => scrollWrites.push({ offset, adjustments }),
    observeElementRect: (_instance, cb) => cb({ width: 390, height: VIEWPORT_PX }),
    observeElementOffset: (_instance, cb) => {
      reportOffset = cb;
      cb(READER_OFFSET, true);
    },
  };
  const virtualizer = new Virtualizer(options);
  virtualizer._willUpdate();
  virtualizer.getTotalSize();
  scrollWrites.length = 0;
  return {
    virtualizer,
    scrollWrites,
    /** The list stops scrolling; the virtualizer applies what it held back. */
    settle: () => reportOffset(READER_OFFSET, false),
  };
}

describe("discardDeferredScrollAdjustment", () => {
  it("holds back a row shift while scrolling and applies it once scrolling settles", () => {
    // Pins the virtualizer behavior the discard targets; fails if it changes.
    const { virtualizer, scrollWrites, settle } = createScrollingVirtualizer();
    virtualizer.resizeItem(5, ROW_PX + 300);
    expect(scrollWrites).toEqual([]);

    settle();
    expect(scrollWrites.at(-1)?.adjustments).toBe(300);
  });

  it("drops the held-back shift after the anchor was corrected against the DOM", () => {
    const { virtualizer, scrollWrites, settle } = createScrollingVirtualizer();
    virtualizer.resizeItem(5, ROW_PX + 300);

    // The anchor correction already moved the reader to the right place.
    discardDeferredScrollAdjustment(virtualizer);
    settle();
    expect(scrollWrites.filter((write) => write.adjustments)).toEqual([]);

    // Rows measured after the correction are still held back and applied.
    virtualizer.isScrolling = true;
    virtualizer.resizeItem(6, ROW_PX + 120);
    settle();
    expect(scrollWrites.at(-1)?.adjustments).toBe(120);
  });
});

describe("shouldAdjustScrollForResizedRow on iOS", () => {
  it("counts shifts already written to the list but not yet reported back", () => {
    const { virtualizer, scrollWrites, settle } = createScrollingVirtualizer();
    virtualizer.shouldAdjustScrollPositionOnItemSizeChange = shouldAdjustScrollForResizedRow;
    virtualizer.resizeItem(5, ROW_PX + 300);
    // Scrolling stops: the held-back 300 is written to the list, while the
    // reported offset stays at 2000 until the next scroll event.
    settle();
    expect(scrollWrites.at(-1)?.adjustments).toBe(300);

    // Row 18 now starts at 2100: above the reader at 2300, so its growth
    // must be compensated too, or the reader is pushed down by it.
    virtualizer.resizeItem(18, ROW_PX + 100);
    expect(scrollWrites.at(-1)?.adjustments).toBe(400);
  });
});
