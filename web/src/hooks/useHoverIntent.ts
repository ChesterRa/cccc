import { useCallback, useEffect, useRef } from "react";

export type HoverIntentOptions = {
  /** Guards against menus flashing open while the pointer merely crosses the trigger. */
  openDelayMs?: number;
  /** Long enough to cross the gap between the trigger and the panel it opened. */
  closeDelayMs?: number;
};

/**
 * Hover-to-open timing for a menu whose open state is owned by the caller.
 *
 * Only one timer is ever pending, so a pointer that leaves and re-enters resolves to the last
 * intent instead of queueing both.
 */
export function useHoverIntent(
  setOpen: (open: boolean) => void,
  { openDelayMs = 120, closeDelayMs = 220 }: HoverIntentOptions = {},
) {
  const timer = useRef<number | null>(null);

  const cancel = useCallback(() => {
    if (timer.current === null) return;
    window.clearTimeout(timer.current);
    timer.current = null;
  }, []);

  const schedule = useCallback(
    (open: boolean, delayMs: number) => {
      cancel();
      timer.current = window.setTimeout(() => {
        timer.current = null;
        setOpen(open);
      }, delayMs);
    },
    [cancel, setOpen],
  );

  const scheduleOpen = useCallback(() => schedule(true, openDelayMs), [schedule, openDelayMs]);
  const scheduleClose = useCallback(() => schedule(false, closeDelayMs), [schedule, closeDelayMs]);

  useEffect(() => cancel, [cancel]);

  return { scheduleOpen, scheduleClose, cancel };
}

/** Touch and pen taps also emit pointerenter; only a mouse should trigger hover intent. */
export function isMousePointer(event: { pointerType?: string }): boolean {
  return event.pointerType === "mouse";
}
