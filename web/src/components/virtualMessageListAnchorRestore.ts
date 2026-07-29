import { useCallback, useMemo, useRef } from "react";
import type { MutableRefObject, RefObject } from "react";

export const INITIAL_ANCHOR_LOCK_MAX_AGE_MS = 2_000;

export function getMessageAnchorOffset(scrollTop: number, itemStart: number): number {
  return (Number(scrollTop) || 0) - (Number(itemStart) || 0);
}

export function getScrollOffsetForMessageAnchor(itemStart: number, offsetPx: number): number {
  return Math.max(0, (Number(itemStart) || 0) + (Number(offsetPx) || 0));
}

export function getInitialAnchorCorrection(input: {
  currentScrollTop: number;
  lockedAnchorTop: number;
  currentAnchorTop: number;
  now: number;
  expiresAt: number;
  minDeltaPx?: number;
}): { active: boolean; scrollTop: number } {
  const currentScrollTop = Number(input.currentScrollTop) || 0;
  if (input.now > input.expiresAt) return { active: false, scrollTop: currentScrollTop };

  const delta = (Number(input.currentAnchorTop) || 0) - (Number(input.lockedAnchorTop) || 0);
  const minDeltaPx = Math.max(0, Number(input.minDeltaPx ?? 0.5) || 0);
  if (Math.abs(delta) <= minDeltaPx) return { active: true, scrollTop: currentScrollTop };
  return { active: true, scrollTop: Math.max(0, currentScrollTop + delta) };
}

type InitialAnchorLock = { anchorId: string; lockedTop: number | null; expiresAt: number };

export function useInitialAnchorRestoreController(input: {
  parentRef: RefObject<HTMLDivElement | null>;
  lastScrollTopRef: MutableRefObject<number>;
  getMessageRowById: (eventId: string) => HTMLDivElement | null;
}) {
  const { parentRef, lastScrollTopRef, getMessageRowById } = input;
  const lockRef = useRef<InitialAnchorLock | null>(null);
  const rafRef = useRef<number | null>(null);
  const remainingChecksRef = useRef(0);

  const cancelScheduledCorrection = useCallback(() => {
    if (rafRef.current != null) {
      window.cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    remainingChecksRef.current = 0;
  }, []);

  const cancel = useCallback(() => {
    lockRef.current = null;
    cancelScheduledCorrection();
  }, [cancelScheduledCorrection]);

  const isActive = useCallback(() => {
    const lock = lockRef.current;
    if (!lock) return false;
    if (performance.now() <= lock.expiresAt) return true;
    lockRef.current = null;
    cancelScheduledCorrection();
    return false;
  }, [cancelScheduledCorrection]);

  const requestCorrection = useCallback(
    (checks = 2) => {
      const lock = lockRef.current;
      if (!lock || performance.now() > lock.expiresAt) {
        cancel();
        return;
      }

      remainingChecksRef.current = Math.max(
        remainingChecksRef.current,
        Math.max(1, Math.round(Number(checks) || 1)),
      );
      if (rafRef.current != null) return;

      const correct = () => {
        rafRef.current = null;
        const currentLock = lockRef.current;
        const now = performance.now();
        if (!currentLock || now > currentLock.expiresAt) {
          cancel();
          return;
        }

        const el = parentRef.current;
        const row = getMessageRowById(currentLock.anchorId);
        if (el && row) {
          const currentAnchorTop = row.getBoundingClientRect().top;
          if (currentLock.lockedTop == null) {
            currentLock.lockedTop = currentAnchorTop;
          } else {
            const correction = getInitialAnchorCorrection({
              currentScrollTop: el.scrollTop,
              lockedAnchorTop: currentLock.lockedTop,
              currentAnchorTop,
              now,
              expiresAt: currentLock.expiresAt,
            });
            if (!correction.active) {
              cancel();
              return;
            }
            if (correction.scrollTop !== el.scrollTop) {
              el.scrollTop = correction.scrollTop;
            }
            lastScrollTopRef.current = el.scrollTop;
          }
        }

        remainingChecksRef.current -= 1;
        if (remainingChecksRef.current > 0) {
          rafRef.current = window.requestAnimationFrame(correct);
        }
      };

      rafRef.current = window.requestAnimationFrame(correct);
    },
    [cancel, getMessageRowById, lastScrollTopRef, parentRef],
  );

  const start = useCallback(
    (anchorId: string) => {
      const normalizedAnchorId = String(anchorId || "").trim();
      if (!normalizedAnchorId) {
        cancel();
        return;
      }
      cancelScheduledCorrection();
      lockRef.current = {
        anchorId: normalizedAnchorId,
        lockedTop: null,
        expiresAt: performance.now() + INITIAL_ANCHOR_LOCK_MAX_AGE_MS,
      };
      requestCorrection(4);
    },
    [cancel, cancelScheduledCorrection, requestCorrection],
  );

  return useMemo(
    () => ({ start, requestCorrection, isActive, cancel }),
    [cancel, isActive, requestCorrection, start],
  );
}
