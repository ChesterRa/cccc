export const PRESENTATION_SPLIT_DEFAULT_WIDTH = 780;
export const PRESENTATION_SPLIT_RAIL_WIDTH = 72;
export const PRESENTATION_SPLIT_DIVIDER_WIDTH = 16;
export const PRESENTATION_SPLIT_MIN_VIEWER_WIDTH = 300;
export const PRESENTATION_SPLIT_MIN_CHAT_WIDTH = 300;

export function clampPresentationSplitWidth(value: number, containerWidth?: number | null): number {
  const numeric = Number(value);
  const fallback = PRESENTATION_SPLIT_DEFAULT_WIDTH;
  const rounded = Number.isFinite(numeric) ? Math.round(numeric) : fallback;
  const minWidth = PRESENTATION_SPLIT_RAIL_WIDTH + PRESENTATION_SPLIT_MIN_VIEWER_WIDTH;

  const totalWidth = Number(containerWidth);
  if (!Number.isFinite(totalWidth) || totalWidth <= 0) {
    return Math.max(minWidth, rounded);
  }

  const maxWidth = Math.max(
    minWidth,
    Math.round(totalWidth - PRESENTATION_SPLIT_DIVIDER_WIDTH - PRESENTATION_SPLIT_MIN_CHAT_WIDTH),
  );
  return Math.min(maxWidth, Math.max(minWidth, rounded));
}
