// Include each pane's title bar; leave room for about 70 columns.
const MIN_TERMINAL_PANE_WIDTH = 560;
const GRID_SPACING = 1; // Shared divider, without outer padding or gaps.

// A page holds at most two panes, side by side at the full height: with many members a
// 2x2 grid leaves each terminal too small to read, so the rest are a page turn away.
const MAX_PANES = 2;

function capacityFor(width: number) {
  return width > 0 && width < MIN_TERMINAL_PANE_WIDTH * 2 + GRID_SPACING ? 1 : MAX_PANES;
}

export function terminalPageLayout(actorCount: number, preferredPage: number, width: number) {
  const pageSize = capacityFor(width);
  const pageCount = Math.max(1, Math.ceil(actorCount / pageSize));
  const page = Math.min(Math.max(0, preferredPage), pageCount - 1);
  return { page, pageCount, pageSize, start: page * pageSize };
}
