import type { VirtualItem, Virtualizer } from "@tanstack/react-virtual";

type MessageListVirtualizer = Virtualizer<HTMLDivElement, Element>;

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
    start < effectiveScrollOffset(virtualizer) &&
    (firstMeasurement || virtualizer.scrollDirection !== "backward")
  );
}

/**
 * Where the list is scrolled to once the shifts already written to it land.
 * After the virtualizer applies held-back iOS shifts, the scroll element has
 * moved but `scrollOffset` only catches up on the next scroll event; until then
 * the pending shifts sit in `scrollAdjustments`, which the default rule adds
 * too. Like the iOS accumulator below it is not public API; the tests pin it.
 */
function effectiveScrollOffset(virtualizer: MessageListVirtualizer): number {
  const pending = (virtualizer as unknown as { scrollAdjustments?: unknown }).scrollAdjustments;
  return (virtualizer.scrollOffset ?? 0) + (typeof pending === "number" ? pending : 0);
}

/**
 * Drops the scroll shift the virtualizer is holding back for iOS.
 *
 * On iOS (and Mac browsers reporting touch points) the virtualizer does not
 * move the scroll while the list is scrolling or touched; it accumulates row
 * height shifts and applies them once scrolling settles. After an anchor was
 * corrected against its real on-screen position, everything measured so far
 * is already accounted for, so applying the held-back shift would push the
 * reader away by the same amount again. Shifts from later measurements are
 * still held back and applied as usual.
 *
 * The accumulator is not public API; the test pins this contract so that a
 * virtualizer upgrade that renames it fails loudly instead of silently.
 */
export function discardDeferredScrollAdjustment(virtualizer: MessageListVirtualizer): void {
  const internals = virtualizer as unknown as { _iosDeferredAdjustment?: unknown };
  if (typeof internals._iosDeferredAdjustment === "number") internals._iosDeferredAdjustment = 0;
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
