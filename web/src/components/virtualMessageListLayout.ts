const HISTORY_STATUS_TOP_SPACE_PX = 56;

export function getNonVirtualMessageListTopMargin({
  topInset,
  showHistoryStatus,
}: {
  topInset: number;
  showHistoryStatus: boolean;
}): number {
  return Math.max(0, Number(topInset) || 0) + (showHistoryStatus ? HISTORY_STATUS_TOP_SPACE_PX : 0);
}
