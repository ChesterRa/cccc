const HISTORY_STATUS_TOP_SPACE_PX = 56;

/**
 * Short lists always reserve the history status badge space. The badge appears
 * while history loads, and the scroll container disables overflow anchoring, so
 * a margin that followed the badge would shift every row when a load starts.
 */
export function getNonVirtualMessageListTopMargin({ topInset }: { topInset: number }): number {
  return Math.max(0, Number(topInset) || 0) + HISTORY_STATUS_TOP_SPACE_PX;
}
