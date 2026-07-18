import { useCallback, useMemo, useRef } from "react";
import type { MutableRefObject, RefObject } from "react";

export type PendingPrependCompensation = {
  previousOffset: number;
  previousTotalSize: number;
  anchorId: string;
  anchorOffsetPx: number;
  anchorTop: number | null;
};

export function shouldTriggerTopHistoryLoad(input: {
  scrollTop: number;
  topTriggerPx: number;
  topLoadArmed: boolean;
  hasMoreHistory: boolean;
  isLoadingHistory: boolean;
  isPrependCompensating: boolean;
  hasLoadMoreHandler: boolean;
}): boolean {
  if (input.isPrependCompensating) return false;
  if (
    !input.topLoadArmed ||
    !input.hasMoreHistory ||
    input.isLoadingHistory ||
    !input.hasLoadMoreHandler
  ) {
    return false;
  }
  return input.scrollTop < input.topTriggerPx;
}

export function shouldRearmTopHistoryLoad(input: {
  scrollTop: number;
  topRearmPx: number;
  isPrependCompensating: boolean;
}): boolean {
  if (input.isPrependCompensating) return false;
  return input.scrollTop > input.topRearmPx;
}

export function getTopHistoryLoadDecision(input: {
  scrollTop: number;
  topTriggerPx: number;
  topRearmPx: number;
  topLoadArmed: boolean;
  hasMoreHistory: boolean;
  isLoadingHistory: boolean;
  isPrependCompensating: boolean;
  hasLoadMoreHandler: boolean;
}): { topLoadArmed: boolean; shouldLoad: boolean } {
  const nextTopLoadArmed = shouldRearmTopHistoryLoad({
    scrollTop: input.scrollTop,
    topRearmPx: input.topRearmPx,
    isPrependCompensating: input.isPrependCompensating,
  })
    ? true
    : input.topLoadArmed;

  const shouldLoad = shouldTriggerTopHistoryLoad({
    scrollTop: input.scrollTop,
    topTriggerPx: input.topTriggerPx,
    topLoadArmed: nextTopLoadArmed,
    hasMoreHistory: input.hasMoreHistory,
    isLoadingHistory: input.isLoadingHistory,
    isPrependCompensating: input.isPrependCompensating,
    hasLoadMoreHandler: input.hasLoadMoreHandler,
  });

  return { topLoadArmed: shouldLoad ? false : nextTopLoadArmed, shouldLoad };
}

export function getCorrectedScrollTopForAnchor(input: {
  currentScrollTop: number;
  lockedAnchorTop: number;
  currentAnchorTop: number;
  minDeltaPx?: number;
}): number {
  const currentScrollTop = Number(input.currentScrollTop) || 0;
  const delta = (Number(input.currentAnchorTop) || 0) - (Number(input.lockedAnchorTop) || 0);
  const minDeltaPx = Math.max(0, Number(input.minDeltaPx ?? 0) || 0);
  if (Math.abs(delta) <= minDeltaPx) return currentScrollTop;
  return currentScrollTop + delta;
}

export function usePrependCompensationController(input: {
  parentRef: RefObject<HTMLDivElement | null>;
  lastScrollTopRef: MutableRefObject<number>;
  getMessageRowById: (eventId: string) => HTMLDivElement | null;
  isVirtualized: boolean;
  scrollToVirtualOffset: (offsetPx: number) => void;
}) {
  const { parentRef, lastScrollTopRef, getMessageRowById, isVirtualized, scrollToVirtualOffset } =
    input;
  const pendingRef = useRef<PendingPrependCompensation | null>(null);
  const isCompensatingRef = useRef(false);
  const rafRef = useRef<number | null>(null);

  const cancelCorrection = useCallback(() => {
    if (rafRef.current != null) {
      window.cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
  }, []);

  const clear = useCallback(() => {
    pendingRef.current = null;
    isCompensatingRef.current = false;
    cancelCorrection();
  }, [cancelCorrection]);

  const begin = useCallback((pending: PendingPrependCompensation) => {
    pendingRef.current = pending;
    isCompensatingRef.current = true;
  }, []);

  const takePending = useCallback(() => {
    const pending = pendingRef.current;
    pendingRef.current = null;
    return pending;
  }, []);

  const finish = useCallback(() => {
    isCompensatingRef.current = false;
  }, []);

  const scrollToOffset = useCallback(
    (offsetPx: number) => {
      const el = parentRef.current;
      if (!el) return;
      const top = Math.max(0, Number(offsetPx) || 0);
      if (isVirtualized) {
        scrollToVirtualOffset(top);
      } else {
        el.scrollTo({ top, behavior: "auto" });
      }
      lastScrollTopRef.current = top;
    },
    [isVirtualized, lastScrollTopRef, parentRef, scrollToVirtualOffset],
  );

  const scheduleAnchorCorrection = useCallback(
    (anchorId: string, lockedAnchorTop: number | null) => {
      cancelCorrection();
      if (!anchorId || lockedAnchorTop == null) {
        finish();
        return;
      }

      let remainingChecks = 2;
      const correctAnchor = () => {
        rafRef.current = null;
        const el = parentRef.current;
        const row = getMessageRowById(anchorId);
        if (el && row) {
          const correctedTop = getCorrectedScrollTopForAnchor({
            currentScrollTop: el.scrollTop,
            lockedAnchorTop,
            currentAnchorTop: row.getBoundingClientRect().top,
            minDeltaPx: 0.5,
          });
          if (correctedTop !== el.scrollTop) {
            el.scrollTop = correctedTop;
          }
          lastScrollTopRef.current = el.scrollTop;
        }

        remainingChecks -= 1;
        if (remainingChecks > 0) {
          rafRef.current = window.requestAnimationFrame(correctAnchor);
          return;
        }
        finish();
      };

      rafRef.current = window.requestAnimationFrame(correctAnchor);
    },
    [cancelCorrection, finish, getMessageRowById, lastScrollTopRef, parentRef],
  );

  return useMemo(
    () => ({
      pendingRef,
      isCompensatingRef,
      begin,
      takePending,
      finish,
      clear,
      cancelCorrection,
      scrollToOffset,
      scheduleAnchorCorrection,
    }),
    [begin, cancelCorrection, clear, finish, scheduleAnchorCorrection, scrollToOffset, takePending],
  );
}

export function useTopHistoryLoadCoordinator(input: {
  compensation: ReturnType<typeof usePrependCompensationController>;
  getAnchorSnapshot: (scrollTop: number) => { anchorId: string; offsetPx: number } | null;
  getAnchorTop: (anchorId: string) => number | null;
  getCurrentContentSize: () => number;
  scrollToMessageAnchor: (eventId: string, offsetPx?: number) => boolean;
  cancelPendingBottomScroll: () => void;
  detachFollowMode: () => void;
  markAwayFromBottom: () => void;
  onLoadMore?: () => void;
}) {
  const {
    compensation,
    getAnchorSnapshot,
    getAnchorTop,
    getCurrentContentSize,
    scrollToMessageAnchor,
    cancelPendingBottomScroll,
    detachFollowMode,
    markAwayFromBottom,
    onLoadMore,
  } = input;
  const topLoadArmedRef = useRef(true);

  const reset = useCallback(() => {
    topLoadArmedRef.current = true;
    compensation.clear();
  }, [compensation]);

  const handleTopHistoryScroll = useCallback(
    (params: {
      scrollTop: number;
      topTriggerPx: number;
      topRearmPx: number;
      hasMoreHistory: boolean;
      isLoadingHistory: boolean;
    }) => {
      const decision = getTopHistoryLoadDecision({
        ...params,
        topLoadArmed: topLoadArmedRef.current,
        isPrependCompensating: compensation.isCompensatingRef.current,
        hasLoadMoreHandler: !!onLoadMore,
      });
      topLoadArmedRef.current = decision.topLoadArmed;
      if (!decision.shouldLoad) return false;

      detachFollowMode();
      markAwayFromBottom();
      cancelPendingBottomScroll();

      const anchor = getAnchorSnapshot(params.scrollTop);
      compensation.begin({
        previousOffset: params.scrollTop,
        previousTotalSize: getCurrentContentSize(),
        anchorId: anchor?.anchorId || "",
        anchorOffsetPx: Number(anchor?.offsetPx || 0),
        anchorTop: anchor?.anchorId ? getAnchorTop(anchor.anchorId) : null,
      });

      onLoadMore?.();
      return true;
    },
    [
      cancelPendingBottomScroll,
      compensation,
      detachFollowMode,
      getAnchorSnapshot,
      getAnchorTop,
      getCurrentContentSize,
      markAwayFromBottom,
      onLoadMore,
    ],
  );

  const applyPendingPrependCompensation = useCallback(
    (params: { isLoadingHistory: boolean }) => {
      if (params.isLoadingHistory) return false;
      const pending = compensation.pendingRef.current;
      if (!pending) return false;

      compensation.takePending();

      if (pending.anchorId && scrollToMessageAnchor(pending.anchorId, pending.anchorOffsetPx)) {
        topLoadArmedRef.current = false;
        compensation.scheduleAnchorCorrection(pending.anchorId, pending.anchorTop);
        return true;
      }

      const nextTotalSize = getCurrentContentSize();
      const delta = Math.max(0, nextTotalSize - pending.previousTotalSize);
      if (delta <= 0) {
        compensation.finish();
        return true;
      }

      compensation.scrollToOffset(pending.previousOffset + delta);
      topLoadArmedRef.current = false;
      compensation.scheduleAnchorCorrection(pending.anchorId, pending.anchorTop);
      return true;
    },
    [compensation, getCurrentContentSize, scrollToMessageAnchor],
  );

  return useMemo(
    () => ({ handleTopHistoryScroll, applyPendingPrependCompensation, reset }),
    [applyPendingPrependCompensation, handleTopHistoryScroll, reset],
  );
}
