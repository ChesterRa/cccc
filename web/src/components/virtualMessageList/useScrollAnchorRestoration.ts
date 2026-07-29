import { useCallback, useLayoutEffect, useMemo, useRef } from "react";

export type RestoredScrollAnchor = { anchorId: string; offsetPx: number };

const RESTORATION_WINDOW_MS = 5_000;

export function getScrollRestorationRequestKey(input: {
  targetId?: string;
  anchorId?: string;
  offsetPx?: number;
}): string {
  const targetId = String(input.targetId || "").trim();
  if (targetId) return `target:${targetId}`;
  const anchorId = String(input.anchorId || "").trim();
  if (!anchorId) return "bottom";
  const offsetPx = Math.round((Number(input.offsetPx) || 0) * 2) / 2;
  return `anchor:${anchorId}:${offsetPx}`;
}

export function useScrollAnchorRestoration(applyAnchor: (anchor: RestoredScrollAnchor) => boolean) {
  const applyAnchorRef = useRef(applyAnchor);
  const activeRef = useRef<(RestoredScrollAnchor & { expiresAt: number }) | null>(null);
  const correctionRafRef = useRef<number | null>(null);

  useLayoutEffect(() => {
    applyAnchorRef.current = applyAnchor;
  }, [applyAnchor]);

  const cancel = useCallback(() => {
    activeRef.current = null;
    if (correctionRafRef.current != null) {
      window.cancelAnimationFrame(correctionRafRef.current);
      correctionRafRef.current = null;
    }
  }, []);

  const correct = useCallback(() => {
    const active = activeRef.current;
    if (!active) return;
    if (performance.now() >= active.expiresAt) {
      cancel();
      return;
    }
    if (correctionRafRef.current != null) return;
    const runCorrection = () => {
      correctionRafRef.current = null;
      const latest = activeRef.current;
      if (!latest || performance.now() >= latest.expiresAt) {
        cancel();
        return;
      }
      applyAnchorRef.current(latest);
      correctionRafRef.current = window.requestAnimationFrame(runCorrection);
    };
    correctionRafRef.current = window.requestAnimationFrame(runCorrection);
  }, [cancel]);

  const begin = useCallback(
    (anchor: RestoredScrollAnchor) => {
      const anchorId = String(anchor.anchorId || "").trim();
      if (!anchorId) {
        cancel();
        return false;
      }
      activeRef.current = {
        anchorId,
        offsetPx: Number(anchor.offsetPx) || 0,
        expiresAt: performance.now() + RESTORATION_WINDOW_MS,
      };
      const applied = applyAnchorRef.current(activeRef.current);
      correct();
      return applied;
    },
    [cancel, correct],
  );

  const isActive = useCallback(() => {
    const active = activeRef.current;
    return !!active && performance.now() < active.expiresAt;
  }, []);

  useLayoutEffect(() => cancel, [cancel]);

  return useMemo(() => ({ begin, cancel, correct, isActive }), [begin, cancel, correct, isActive]);
}
