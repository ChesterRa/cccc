// @vitest-environment happy-dom

import { Virtualizer, type VirtualizerOptions } from "@tanstack/react-virtual";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { getLayoutOffsetForScrollTop, shouldAdjustScrollForResizedRow } from "./virtualOffset";

beforeAll(() => {
  Object.defineProperty(window.navigator, "userAgent", {
    configurable: true,
    value: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15",
  });
});
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

const ROW_PX = 100;
const VIEWPORT_PX = 600;
const READER_OFFSET = 2_000;
const ANCHOR_INDEX = 20;

function createScrollingVirtualizer() {
  const element = document.createElement("div");
  Object.defineProperty(element, "scrollHeight", { configurable: true, value: 100_000 });
  Object.defineProperty(element, "clientHeight", { configurable: true, value: VIEWPORT_PX });
  element.scrollTop = READER_OFFSET;
  const writes: number[] = [];
  let report: (offset: number, isScrolling: boolean) => void = () => {};
  const options: VirtualizerOptions<HTMLDivElement, Element> = {
    count: 50,
    getScrollElement: () => element,
    estimateSize: () => ROW_PX,
    scrollToFn: (offset, { adjustments }) => {
      element.scrollTop = offset + (adjustments ?? 0);
      writes.push(element.scrollTop);
    },
    observeElementRect: (_instance, cb) => cb({ width: 390, height: VIEWPORT_PX }),
    observeElementOffset: (_instance, cb) => {
      report = cb;
      cb(element.scrollTop, false);
    },
  };
  const virtualizer = new Virtualizer(options);
  virtualizer._willUpdate();
  virtualizer.getTotalSize();
  writes.length = 0;
  const drag = () => {
    element.dispatchEvent(new Event("touchstart"));
    element.scrollTop = READER_OFFSET - 50;
    report(element.scrollTop, true);
  };
  const settle = () => {
    element.dispatchEvent(new Event("touchend"));
    report(element.scrollTop, false);
    vi.advanceTimersByTime(500);
    report(element.scrollTop, false);
  };
  const anchorTop = () => {
    const row = virtualizer.getVirtualItems().find((item) => item.index === ANCHOR_INDEX);
    if (!row) throw new Error("anchor not rendered");
    return row.start - element.scrollTop;
  };
  return { virtualizer, element, writes, report, drag, settle, anchorTop };
}

describe("iOS virtual message positioning", () => {
  it("keeps the visible anchor still while a measured row changes above it", () => {
    const { virtualizer, writes, drag, settle, anchorTop } = createScrollingVirtualizer();
    drag();
    const before = anchorTop();
    virtualizer.resizeItem(5, ROW_PX + 300);
    expect(writes).toEqual([]);
    expect(anchorTop()).toBe(before);
    settle();
    expect(anchorTop()).toBe(before);
  });

  it("keeps an explicitly corrected anchor after held measurements settle", () => {
    const { virtualizer, element, report, drag, settle, anchorTop } = createScrollingVirtualizer();
    drag();
    virtualizer.resizeItem(5, ROW_PX + 300);
    virtualizer.scrollToOffset(getLayoutOffsetForScrollTop(virtualizer, READER_OFFSET));
    report(element.scrollTop, true);
    settle();
    expect(anchorTop()).toBe(0);
  });

  it("keeps the anchor stable across later measurements above the viewport", () => {
    const { virtualizer, drag, settle, anchorTop } = createScrollingVirtualizer();
    virtualizer.shouldAdjustScrollPositionOnItemSizeChange = shouldAdjustScrollForResizedRow;
    drag();
    const before = anchorTop();
    virtualizer.resizeItem(5, ROW_PX + 300);
    settle();
    virtualizer.resizeItem(6, ROW_PX + 100);
    expect(anchorTop()).toBe(before);
  });
});
