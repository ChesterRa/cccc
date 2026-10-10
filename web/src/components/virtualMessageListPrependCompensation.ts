import { useCallback, useMemo, useRef } from "react";
import type { MutableRefObject, RefObject } from "react";

/** Upper bound on frames spent settling the anchor after a history prepend. */
const ANCHOR_CORRECTION_MAX_FRAMES = 8;
/** Consecutive frames without a correction that count as settled. */
const ANCHOR_CORRECTION_STABLE_FRAMES = 2;
/** Frames the virtualizer gets to bring a missing anchor back on its own. */
const ANCHOR_REVEAL_AFTER_MISSING_FRAMES = 2;

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
  /** Called after the anchor was checked against its real on-screen position. */
  onAnchorVerified?: () => void;
}) {
  const {
    parentRef,
    lastScrollTopRef,
    getMessageRowById,
    isVirtualized,
    scrollToVirtualOffset,
    onAnchorVerified,
  } = input;
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
    (anchorId: string, lockedAnchorTop: number | null, revealAnchor?: () => void) => {
      cancelCorrection();
      if (!anchorId || lockedAnchorTop == null) {
        finish();
        return;
      }

      let remainingChecks = ANCHOR_CORRECTION_MAX_FRAMES;
      let stableFrames = 0;
      let missingFrames = 0;
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
            // Through the virtualizer, so this fixed target replaces any
            // index-based scroll it is still reconciling.
            scrollToOffset(correctedTop);
            stableFrames = 0;
          } else {
            stableFrames += 1;
          }
          lastScrollTopRef.current = el.scrollTop;
          // The real position covers every shift measured so far; nothing that
          // was held back before this check may be applied on top of it.
          onAnchorVerified?.();
        } else {
          // Virtual rows above the anchor are measured only once they render.
          // The virtualizer usually shifts the scroll by their real heights and
          // brings the anchor back; when estimates were too far off it does
          // not, so hand over once to a reveal that converges on the row.
          missingFrames += 1;
          if (missingFrames === ANCHOR_REVEAL_AFTER_MISSING_FRAMES) revealAnchor?.();
          stableFrames = 0;
        }

        remainingChecks -= 1;
        if (remainingChecks > 0 && stableFrames < ANCHOR_CORRECTION_STABLE_FRAMES) {
          rafRef.current = window.requestAnimationFrame(correctAnchor);
          return;
        }
        finish();
      };

      rafRef.current = window.requestAnimationFrame(correctAnchor);
    },
    [
      cancelCorrection,
      finish,
      getMessageRowById,
      lastScrollTopRef,
      onAnchorVerified,
      parentRef,
      scrollToOffset,
    ],
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
  /** Bring a message row into the rendered range when its offset is unknown. */
  revealMessageAnchor?: (eventId: string) => void;
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
    revealMessageAnchor,
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

  const capturePendingCompensation = useCallback(
    (scrollTop: number): PendingPrependCompensation => {
      const anchor = getAnchorSnapshot(scrollTop);
      return {
        previousOffset: scrollTop,
        previousTotalSize: getCurrentContentSize(),
        anchorId: anchor?.anchorId || "",
        anchorOffsetPx: Number(anchor?.offsetPx || 0),
        anchorTop: anchor?.anchorId ? getAnchorTop(anchor.anchorId) : null,
      };
    },
    [getAnchorSnapshot, getAnchorTop, getCurrentContentSize],
  );

  const handleTopHistoryScroll = useCallback(
    (params: {
      scrollTop: number;
      topTriggerPx: number;
      topRearmPx: number;
      hasMoreHistory: boolean;
      isLoadingHistory: boolean;
    }) => {
      // The reader keeps scrolling (mobile momentum included) while the older
      // page loads. Track where they are now, otherwise the compensation would
      // snap back to the position captured when the load was triggered.
      if (compensation.pendingRef.current) {
        compensation.begin(capturePendingCompensation(params.scrollTop));
        return false;
      }

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

      compensation.begin(capturePendingCompensation(params.scrollTop));

      onLoadMore?.();
      return true;
    },
    [
      cancelPendingBottomScroll,
      capturePendingCompensation,
      compensation,
      detachFollowMode,
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
        compensation.scheduleAnchorCorrection(
          pending.anchorId,
          pending.anchorTop,
          revealMessageAnchor ? () => revealMessageAnchor(pending.anchorId) : undefined,
        );
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
    [compensation, getCurrentContentSize, revealMessageAnchor, scrollToMessageAnchor],
  );

  return useMemo(
    () => ({ handleTopHistoryScroll, applyPendingPrependCompensation, reset }),
    [applyPendingPrependCompensation, handleTopHistoryScroll, reset],
  );
}
