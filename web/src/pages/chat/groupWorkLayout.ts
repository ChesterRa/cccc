/** Terminals sit side by side, one full-height column each. */
const MIN_TERMINAL_COLUMN_WIDTH = 400;
const MAX_TERMINAL_COLUMNS = 4;

function columnsFor(width: number) {
  if (width <= 0) return MAX_TERMINAL_COLUMNS;
  if (width < 720) return 1;
  return Math.min(MAX_TERMINAL_COLUMNS, Math.max(2, Math.floor(width / MIN_TERMINAL_COLUMN_WIDTH)));
}

export function terminalPageLayout(actorCount: number, preferredPage: number, width: number) {
  const pageSize = columnsFor(width);
  const pageCount = Math.max(1, Math.ceil(actorCount / pageSize));
  const page = Math.min(Math.max(0, preferredPage), pageCount - 1);
  return { page, pageCount, pageSize, start: page * pageSize };
}
