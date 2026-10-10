// @vitest-environment happy-dom

import { Virtualizer, type VirtualizerOptions } from "@tanstack/react-virtual";
import { describe, expect, it } from "vite-plus/test";

import { getFreshVirtualOffsetForIndex, shouldAdjustScrollForResizedRow } from "./virtualOffset";

const ESTIMATED_ROW_PX = 100;
const MEASURED_ROW_PX = 1_000;
const VIEWPORT_PX = 600;

/**
 * A scroll container whose scroll range follows the spacer's inline height,
 * like a browser does, so offsets beyond the rendered range are clamped.
 */
function createScrollElement() {
  const scrollElement = document.createElement("div");
  const spacer = document.createElement("div");
  scrollElement.append(spacer);
  Object.defineProperty(scrollElement, "scrollHeight", {
    configurable: true,
    get: () => Math.max(VIEWPORT_PX, Number.parseFloat(spacer.style.height) || 0),
  });
  Object.defineProperty(scrollElement, "clientHeight", { configurable: true, value: VIEWPORT_PX });
  return { scrollElement, spacer };
}

function createListVirtualizer(keys: string[], scrollOffset = 0) {
  const { scrollElement, spacer } = createScrollElement();
  const options = (rowKeys: string[]): VirtualizerOptions<HTMLDivElement, Element> => ({
    count: rowKeys.length,
    getScrollElement: () => scrollElement,
    getItemKey: (index) => rowKeys[index],
    estimateSize: () => ESTIMATED_ROW_PX,
    scrollToFn: () => {},
    observeElementRect: (_instance, cb) => cb({ width: 390, height: VIEWPORT_PX }),
    observeElementOffset: (_instance, cb) => cb(scrollOffset, false),
  });
  const virtualizer = new Virtualizer(options(keys));
  virtualizer._willUpdate();
  // A render: rows are laid out and the spacer gets the total height.
  const render = () => {
    spacer.style.height = `${virtualizer.getTotalSize()}px`;
  };
  render();
  return {
    virtualizer,
    spacer,
    render,
    setKeys: (next: string[]) => virtualizer.setOptions(options(next)),
  };
}

describe("getFreshVirtualOffsetForIndex", () => {
  it("lands on the anchor row after a prepend whose top rows were measured in the commit", () => {
    const loaded = Array.from({ length: 20 }, (_, i) => `loaded-${i}`);
    const { virtualizer, spacer, render, setKeys } = createListVirtualizer(loaded);

    // An older page is prepended while the reader sits at scrollTop 0.
    const older = Array.from({ length: 30 }, (_, i) => `older-${i}`);
    setKeys([...older, ...loaded]);
    render(); // the render pass still sizes every new row by its estimate
    // The commit mounts and measures the new top rows before layout effects run.
    for (let index = 0; index < 10; index += 1) virtualizer.resizeItem(index, MEASURED_ROW_PX);

    const anchorIndex = older.length; // "loaded-0", the row the reader was looking at
    const expectedOffset = 10 * MEASURED_ROW_PX + 20 * ESTIMATED_ROW_PX;
    expect(getFreshVirtualOffsetForIndex(virtualizer, anchorIndex, spacer)).toBe(expectedOffset);
    expect(Number.parseFloat(spacer.style.height)).toBeGreaterThanOrEqual(
      expectedOffset + VIEWPORT_PX,
    );
  });

  it("returns null when the virtualizer has no rows", () => {
    const { virtualizer, spacer } = createListVirtualizer([]);
    expect(getFreshVirtualOffsetForIndex(virtualizer, 0, spacer)).toBeNull();
  });
});

describe("shouldAdjustScrollForResizedRow", () => {
  it("keeps the row at the viewport top still when a batch of rows around it is measured", () => {
    const keys = Array.from({ length: 40 }, (_, i) => `row-${i}`);
    const anchorIndex = 20;
    // The reader's row sits at the viewport top, as after revealing a history anchor.
    const { virtualizer } = createListVirtualizer(keys, anchorIndex * ESTIMATED_ROW_PX);
    virtualizer.shouldAdjustScrollPositionOnItemSizeChange = shouldAdjustScrollForResizedRow;

    // Rows mounted together are measured in one batch, two above the anchor and
    // the anchor plus rows below it; all are taller than their estimates.
    for (let index = anchorIndex - 2; index < anchorIndex + 8; index += 1) {
      virtualizer.resizeItem(index, 3 * ESTIMATED_ROW_PX);
    }

    virtualizer.getTotalSize();
    const anchorStart = virtualizer.measurementsCache[anchorIndex].start;
    // Only the two rows above shift the scroll, so the anchor stays at the top.
    expect(virtualizer.scrollOffset).toBe(
      anchorIndex * ESTIMATED_ROW_PX + 2 * 2 * ESTIMATED_ROW_PX,
    );
    expect(anchorStart - (virtualizer.scrollOffset ?? 0)).toBe(0);
  });
});
