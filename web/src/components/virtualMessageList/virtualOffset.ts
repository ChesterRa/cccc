import type { VirtualItem, Virtualizer } from "@tanstack/react-virtual";

type MessageListVirtualizer = Virtualizer<HTMLDivElement, Element>;

/** Convert a DOM scroll position to the virtualizer's layout coordinate. */
export function getLayoutOffsetForScrollTop(
  virtualizer: MessageListVirtualizer,
  scrollTop: number,
): number {
  const first = virtualizer.getVirtualItems()[0];
  const measured = first ? virtualizer.measurementsCache[first.index] : undefined;
  const held = first && measured ? measured.start - first.start : 0;
  return Math.max(0, scrollTop + held);
}

/**
 * Replacement for the virtualizer's default "keep the view still" rule when a
 * row's measured height changes: shift the scroll only for rows above it.
 *
 * The default compares the row's start from the measurement cache against the
 * scroll offset. Rows that mount together are measured in one batch, so later
 * rows see stale starts while the offset already includes the earlier rows'
 * shifts; rows below the viewport then count as above it and the scroll runs
 * away by thousands of pixels (seen right after a history prepend). Rebuilding
 * the cache first compares current starts. The default's other condition is
 * kept: re-measured rows do not shift the scroll while scrolling backward.
 */
export function shouldAdjustScrollForResizedRow(
  item: VirtualItem,
  _delta: number,
  virtualizer: MessageListVirtualizer,
): boolean {
  virtualizer.getTotalSize();
  const start = virtualizer.measurementsCache[item.index]?.start ?? item.start;
  const firstMeasurement = !virtualizer.itemSizeCache.has(item.key);
  return (
    start < (virtualizer.scrollOffset ?? 0) &&
    (firstMeasurement || virtualizer.scrollDirection !== "backward")
  );
}

/**
 * Scroll offset that puts `index` at the top of the list, from current row sizes.
 *
 * `getOffsetForIndex` reads the virtualizer's measurement cache as-is, but rows
 * measured during the latest commit only reach that cache on its next lazy
 * rebuild. After a history prepend at scrollTop 0 the new top rows are measured
 * in the same commit, so the stale estimates would land the reader thousands of
 * pixels above the anchor. `getTotalSize()` is the public call that rebuilds it.
 *
 * The spacer still has the height from that render, and both the virtualizer
 * and the browser clamp offsets to the current scroll range, so it is synced to
 * the fresh total first. React writes the same height on the next render.
 */
export function getFreshVirtualOffsetForIndex(
  virtualizer: Pick<MessageListVirtualizer, "getTotalSize" | "getOffsetForIndex">,
  index: number,
  spacer: HTMLElement | null,
): number | null {
  const totalSize = virtualizer.getTotalSize();
  if (spacer) spacer.style.height = `${totalSize}px`;
  const offsetInfo = virtualizer.getOffsetForIndex(index, "start");
  return offsetInfo ? offsetInfo[0] : null;
}
