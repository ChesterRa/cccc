// Include each pane's title bar; leave room for about 70 columns and 20 rows.
const MIN_TERMINAL_PANE_WIDTH = 560;
const MIN_TERMINAL_PANE_HEIGHT = 360;
const GRID_SPACING = 1; // Shared divider, without outer padding or gaps.

function capacityFor(width: number, height: number) {
  if (width <= 0 || height <= 0) return 4;
  if (width < MIN_TERMINAL_PANE_WIDTH * 2 + GRID_SPACING) return 1;
  return height >= MIN_TERMINAL_PANE_HEIGHT * 2 + GRID_SPACING ? 4 : 2;
}

export function terminalPageLayout(
  actorCount: number,
  preferredPage: number,
  width: number,
  height: number,
) {
  const pageSize = capacityFor(width, height);
  const pageCount = Math.max(1, Math.ceil(actorCount / pageSize));
  const page = Math.min(Math.max(0, preferredPage), pageCount - 1);
  return { page, pageCount, pageSize, start: page * pageSize };
}
