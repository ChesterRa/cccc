export type VoiceAssistantLoadingOwnership = {
  dataSeq: number;
  visibleLoadingSeq: number;
};

export type VoiceAssistantRefreshOwnership = {
  dataSeq: number;
  visibleLoadingSeq: number;
};

export function beginVoiceAssistantRefresh(
  current: VoiceAssistantLoadingOwnership,
  opts: { quiet: boolean },
): { next: VoiceAssistantLoadingOwnership; request: VoiceAssistantRefreshOwnership; shouldShowLoading: boolean } {
  const dataSeq = current.dataSeq + 1;
  const visibleLoadingSeq = opts.quiet ? current.visibleLoadingSeq : dataSeq;
  return {
    next: { dataSeq, visibleLoadingSeq },
    request: { dataSeq, visibleLoadingSeq: opts.quiet ? 0 : visibleLoadingSeq },
    shouldShowLoading: !opts.quiet,
  };
}

export function shouldApplyVoiceAssistantRefresh(
  current: VoiceAssistantLoadingOwnership,
  request: VoiceAssistantRefreshOwnership,
  isCurrentGroup: boolean,
): boolean {
  return isCurrentGroup && request.dataSeq === current.dataSeq;
}

export function shouldFinishVoiceAssistantVisibleLoading(
  current: VoiceAssistantLoadingOwnership,
  request: VoiceAssistantRefreshOwnership,
  isCurrentGroup: boolean,
): boolean {
  return isCurrentGroup && request.visibleLoadingSeq > 0 && request.visibleLoadingSeq === current.visibleLoadingSeq;
}

export function resetVoiceAssistantVisibleLoading(
  current: VoiceAssistantLoadingOwnership,
): VoiceAssistantLoadingOwnership {
  return {
    dataSeq: current.dataSeq + 1,
    visibleLoadingSeq: 0,
  };
}
