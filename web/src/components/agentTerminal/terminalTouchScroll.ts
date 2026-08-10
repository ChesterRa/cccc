import type { Terminal } from "@xterm/xterm";

const FALLBACK_CELL_HEIGHT_PX = 18;
const TAP_MOVE_THRESHOLD_PX = 8;

function touchClientY(event: TouchEvent): number | null {
  if (event.touches.length !== 1) return null;
  const value = event.touches[0]?.clientY;
  return Number.isFinite(value) ? value : null;
}

function terminalCellHeight(term: Terminal): number {
  const rows = Math.max(1, term.rows || 0);
  const screen = term.element?.querySelector<HTMLElement>(".xterm-screen");
  const height = screen?.getBoundingClientRect().height || term.element?.clientHeight || 0;
  return height > 0 ? Math.max(1, height / rows) : FALLBACK_CELL_HEIGHT_PX;
}

export function attachTerminalTouchScroll(term: Terminal): () => void {
  const element = term.element;
  if (!element) return () => undefined;

  const previousTouchAction = element.style.touchAction;
  const previousOverscrollBehavior = element.style.overscrollBehavior;
  element.style.touchAction = "none";
  element.style.overscrollBehavior = "contain";

  let active = false;
  let startY = 0;
  let lastY = 0;
  let remainderPx = 0;
  let moved = false;

  const reset = () => {
    active = false;
    startY = 0;
    lastY = 0;
    remainderPx = 0;
    moved = false;
  };

  const onTouchStart = (event: TouchEvent) => {
    const clientY = touchClientY(event);
    if (clientY === null) {
      reset();
      return;
    }
    active = true;
    startY = clientY;
    lastY = clientY;
    remainderPx = 0;
    moved = false;
  };

  const onTouchMove = (event: TouchEvent) => {
    if (!active) return;
    const clientY = touchClientY(event);
    if (clientY === null) {
      reset();
      return;
    }

    if (event.cancelable) event.preventDefault();
    const deltaPx = lastY - clientY;
    lastY = clientY;
    if (Math.abs(clientY - startY) >= TAP_MOVE_THRESHOLD_PX) moved = true;

    const cellHeight = terminalCellHeight(term);
    const accumulatedPx = remainderPx + deltaPx;
    const lines = Math.trunc(accumulatedPx / cellHeight);
    remainderPx = accumulatedPx - lines * cellHeight;
    if (lines !== 0) term.scrollLines(lines);
  };

  const onTouchEnd = () => {
    if (!active) return;
    const shouldFocus = !moved;
    reset();
    if (shouldFocus) term.focus();
  };

  element.addEventListener("touchstart", onTouchStart, { passive: true });
  element.addEventListener("touchmove", onTouchMove, { passive: false });
  element.addEventListener("touchend", onTouchEnd, { passive: true });
  element.addEventListener("touchcancel", reset, { passive: true });

  return () => {
    element.removeEventListener("touchstart", onTouchStart);
    element.removeEventListener("touchmove", onTouchMove);
    element.removeEventListener("touchend", onTouchEnd);
    element.removeEventListener("touchcancel", reset);
    element.style.touchAction = previousTouchAction;
    element.style.overscrollBehavior = previousOverscrollBehavior;
  };
}
