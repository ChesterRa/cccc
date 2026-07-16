const MAX_CACHED_VIEWS = 24;
const MAX_ROWS_PER_VIEW = 1_000;

const rowHeightsByView = new Map<string, Map<string | number, number>>();

function getOrCreateView(viewKey: string): Map<string | number, number> {
  const key = String(viewKey || "").trim();
  const existing = rowHeightsByView.get(key);
  if (existing) {
    rowHeightsByView.delete(key);
    rowHeightsByView.set(key, existing);
    return existing;
  }
  const created = new Map<string | number, number>();
  rowHeightsByView.set(key, created);
  while (rowHeightsByView.size > MAX_CACHED_VIEWS) {
    const oldest = rowHeightsByView.keys().next().value;
    if (oldest === undefined) break;
    rowHeightsByView.delete(oldest);
  }
  return created;
}

export function getCachedMessageRowHeight(
  viewKey: string,
  messageKey: string | number,
): number | undefined {
  return rowHeightsByView.get(String(viewKey || "").trim())?.get(messageKey);
}

export function cacheMessageRowHeight(
  viewKey: string,
  messageKey: string | number,
  height: number,
): void {
  const normalized = Math.round(Number(height) || 0);
  if (normalized <= 0) return;
  const rows = getOrCreateView(viewKey);
  rows.delete(messageKey);
  rows.set(messageKey, normalized);
  while (rows.size > MAX_ROWS_PER_VIEW) {
    const oldest = rows.keys().next().value;
    if (oldest === undefined) break;
    rows.delete(oldest);
  }
}
