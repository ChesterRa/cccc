export const SIDE_PANEL_DEFAULT_WIDTH = 360;
export const SIDE_PANEL_MIN_WIDTH = 280;
export const SIDE_PANEL_COMPACT_WIDTH = 64;
export const SIDE_PANEL_DIVIDER_WIDTH = 8;
export const SIDE_PANEL_MIN_WORK_WIDTH = 320;

export function clampSidePanelWidth(value: number, containerWidth?: number): number {
  const width = Number.isFinite(value) && value > 0 ? Math.round(value) : SIDE_PANEL_DEFAULT_WIDTH;
  if (!containerWidth || !Number.isFinite(containerWidth))
    return Math.max(SIDE_PANEL_MIN_WIDTH, width);
  const max = Math.max(
    SIDE_PANEL_COMPACT_WIDTH,
    containerWidth - SIDE_PANEL_DIVIDER_WIDTH - SIDE_PANEL_MIN_WORK_WIDTH,
  );
  return Math.min(max, Math.max(Math.min(SIDE_PANEL_MIN_WIDTH, max), width));
}

/** A deliberate gap between collapse and expansion prevents jitter at the boundary. */
export function shouldCompactSidePanel(width: number, wasCompact: boolean): boolean {
  return width < (wasCompact ? 220 : 180);
}
