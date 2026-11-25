import type { Terminal } from "@xterm/xterm";

const FALLBACK_CELL_HEIGHT_PX = 18;
const TAP_MOVE_THRESHOLD_PX = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function touchPoint(event: TouchEvent): { x: number; y: number } | null {
  if (event.touches.length !== 1) return null;
  const touch = event.touches[0];
  if (!touch || !Number.isFinite(touch.clientY)) return null;
  return { x: Number.isFinite(touch.clientX) ? touch.clientX : 0, y: touch.clientY };
}

function terminalCellHeight(term: Terminal): number {
  const rows = Math.max(1, term.rows || 0);
  const screen = term.element?.querySelector<HTMLElement>(".xterm-screen");
  const height = screen?.getBoundingClientRect().height || term.element?.clientHeight || 0;
  return height > 0 ? Math.max(1, height / rows) : FALLBACK_CELL_HEIGHT_PX;
}

function isScrollableOverflow(value: string): boolean {
  return value === "auto" || value === "scroll";
}

function maxContainerScroll(container: HTMLElement | null): { x: number; y: number } {
  if (!container) return { x: 0, y: 0 };
  const style = window.getComputedStyle(container);
  const overflowX = style.overflowX || container.style.overflowX || container.style.overflow;
  const overflowY = style.overflowY || container.style.overflowY || container.style.overflow;
  return {
    x: isScrollableOverflow(overflowX)
      ? Math.max(0, container.scrollWidth - container.clientWidth)
      : 0,
    y: isScrollableOverflow(overflowY)
      ? Math.max(0, container.scrollHeight - container.clientHeight)
      : 0,
  };
}

function normalBufferScrollState(term: Terminal): { baseY: number; viewportY: number } {
  const active = term.buffer.active as { baseY?: number; viewportY?: number };
  const baseY = Number.isFinite(active.baseY) ? Math.max(0, active.baseY as number) : 0;
  const viewportY = Number.isFinite(active.viewportY)
    ? clamp(active.viewportY as number, 0, baseY)
    : baseY;
  return { baseY, viewportY };
}

function reportsWheelToApplication(term: Terminal): boolean {
  const mouseMode = term.modes.mouseTrackingMode;
  return mouseMode === "vt200" || mouseMode === "drag" || mouseMode === "any";
}

export function attachTerminalTouchScroll(term: Terminal, canSendInput: () => boolean): () => void {
  const element = term.element;
  if (!element) return () => undefined;
  const container = element.parentElement;

  const previousTouchAction = element.style.touchAction;
  const previousOverscrollBehavior = element.style.overscrollBehavior;
  element.style.touchAction = "none";
  element.style.overscrollBehavior = "contain";

  let active = false;
  let startX = 0;
  let lastX = 0;
  let startY = 0;
  let lastY = 0;
  let remainderPx = 0;
  let moved = false;
  let axisLock: "x" | "y" | null = null;

  const reset = () => {
    active = false;
    startX = 0;
    lastX = 0;
    startY = 0;
    lastY = 0;
    remainderPx = 0;
    moved = false;
    axisLock = null;
  };

  // In read-only follower mode, xterm's bubble-phase wheel handlers (mouse tracking and
  // alternate-buffer arrow synthesis) call preventDefault() and block scrolling an overflowing
  // parent container. Stop propagation in capture phase whenever the container should scroll so
  // native wheel, Shift+wheel, and trackpad panning keep working without losing normal scrollback.
  const onWheelCapture = (event: WheelEvent) => {
    if (!container || canSendInput()) return;
    const max = maxContainerScroll(container);
    if (max.x === 0 && max.y === 0) return;

    const shiftHorizontal = event.deltaX === 0 && event.shiftKey && max.x > 0;
    const effectiveDeltaX = shiftHorizontal ? event.deltaY : event.deltaX;
    const effectiveDeltaY = shiftHorizontal ? 0 : event.deltaY;

    const predominantlyHorizontal =
      max.x > 0 && Math.abs(effectiveDeltaX) > Math.abs(effectiveDeltaY);
    const inNormalBuffer = term.buffer.active.type === "normal";
    const { baseY, viewportY } = normalBufferScrollState(term);

    const letXtermScrollHistory =
      !predominantlyHorizontal &&
      effectiveDeltaY !== 0 &&
      inNormalBuffer &&
      !reportsWheelToApplication(term) &&
      baseY > 0 &&
      ((effectiveDeltaY < 0 && container.scrollTop <= 0 && viewportY > 0) ||
        (effectiveDeltaY > 0 && viewportY < baseY));

    if (letXtermScrollHistory) return;

    const wantsX = effectiveDeltaX !== 0 && max.x > 0;
    const wantsY = effectiveDeltaY !== 0 && max.y > 0;
    if (wantsX || wantsY) {
      event.stopPropagation();
    }
  };

  const onTouchStart = (event: TouchEvent) => {
    const point = touchPoint(event);
    if (!point) {
      reset();
      return;
    }
    active = true;
    startX = point.x;
    lastX = point.x;
    startY = point.y;
    lastY = point.y;
    remainderPx = 0;
    moved = false;
    axisLock = null;
  };

  const onTouchMove = (event: TouchEvent) => {
    if (!active) return;
    const point = touchPoint(event);
    if (!point) {
      reset();
      return;
    }

    if (event.cancelable) event.preventDefault();
    const deltaX = lastX - point.x;
    let deltaY = lastY - point.y;
    lastX = point.x;
    lastY = point.y;

    const totalDx = Math.abs(point.x - startX);
    const totalDy = Math.abs(point.y - startY);
    if (totalDx >= TAP_MOVE_THRESHOLD_PX || totalDy >= TAP_MOVE_THRESHOLD_PX) {
      moved = true;
    }

    const readOnly = !canSendInput();
    const max = readOnly ? maxContainerScroll(container) : { x: 0, y: 0 };

    if (container && max.x > 0 && max.y === 0) {
      if (
        axisLock === null &&
        (totalDx >= TAP_MOVE_THRESHOLD_PX || totalDy >= TAP_MOVE_THRESHOLD_PX)
      ) {
        axisLock = totalDx >= totalDy ? "x" : "y";
      }
      if (axisLock === "x" || (axisLock === null && Math.abs(deltaX) >= Math.abs(deltaY))) {
        if (deltaX !== 0) {
          container.scrollLeft = clamp(container.scrollLeft + deltaX, 0, max.x);
        }
        return;
      }
    } else if (container && (max.x > 0 || max.y > 0)) {
      if (max.x > 0 && deltaX !== 0) {
        container.scrollLeft = clamp(container.scrollLeft + deltaX, 0, max.x);
      }
      if (max.y > 0 && deltaY !== 0) {
        if (term.buffer.active.type !== "normal") {
          container.scrollTop = clamp(container.scrollTop + deltaY, 0, max.y);
          remainderPx = 0;
          return;
        }
        const { baseY, viewportY } = normalBufferScrollState(term);
        if (deltaY < 0 && container.scrollTop > 0) {
          const consumed = Math.min(container.scrollTop, -deltaY);
          container.scrollTop -= consumed;
          deltaY += consumed;
          if (deltaY === 0) {
            remainderPx = 0;
            return;
          }
        } else if (deltaY > 0 && viewportY >= baseY) {
          container.scrollTop = Math.min(max.y, container.scrollTop + deltaY);
          remainderPx = 0;
          return;
        }
      }
    }

    const cellHeight = terminalCellHeight(term);
    const accumulatedPx = remainderPx + deltaY;
    const lines = Math.trunc(accumulatedPx / cellHeight);
    remainderPx = accumulatedPx - lines * cellHeight;
    if (lines === 0) return;
    const reportsWheel = reportsWheelToApplication(term);
    // X10 reports button presses only. Its normal buffer still needs local
    // scrollback, since a wheel event on the root reaches neither reporting
    // nor the child viewport's local scroll listener.
    // Mouse tracking is an application request, not proof of writer ownership.
    // Read current permission on every move, including handoffs during a gesture.
    if (
      !term.options.disableStdin &&
      canSendInput() &&
      (reportsWheel || term.buffer.active.type === "alternate")
    ) {
      // xterm listens on element and owns mouse/alternate-buffer encoding.
      // Line units avoid its small-pixel trackpad scaling; one event per line
      // also supports TUIs that consume each wheel report as a single step.
      for (let line = 0; line < Math.abs(lines); line += 1) {
        element.dispatchEvent(
          new WheelEvent("wheel", {
            deltaY: Math.sign(lines),
            deltaMode: WheelEvent.DOM_DELTA_LINE,
            clientX: point.x,
            clientY: point.y,
            bubbles: true,
            cancelable: true,
          }),
        );
      }
    } else if (term.buffer.active.type === "normal") {
      if (container && max.y > 0 && lines > 0) {
        const { baseY, viewportY } = normalBufferScrollState(term);
        const linesToBottom = Math.max(0, baseY - viewportY);
        if (lines > linesToBottom) {
          if (linesToBottom > 0) term.scrollLines(linesToBottom);
          const leftoverPx = accumulatedPx - linesToBottom * cellHeight;
          container.scrollTop = Math.min(max.y, container.scrollTop + Math.max(0, leftoverPx));
          remainderPx = 0;
          return;
        }
      }
      term.scrollLines(lines);
    }
  };

  const onTouchEnd = () => {
    if (!active) return;
    const shouldFocus = !moved;
    reset();
    if (shouldFocus) term.focus();
  };

  container?.addEventListener("wheel", onWheelCapture, { capture: true, passive: true });
  element.addEventListener("touchstart", onTouchStart, { passive: true });
  element.addEventListener("touchmove", onTouchMove, { passive: false });
  element.addEventListener("touchend", onTouchEnd, { passive: true });
  element.addEventListener("touchcancel", reset, { passive: true });

  return () => {
    container?.removeEventListener("wheel", onWheelCapture, { capture: true });
    element.removeEventListener("touchstart", onTouchStart);
    element.removeEventListener("touchmove", onTouchMove);
    element.removeEventListener("touchend", onTouchEnd);
    element.removeEventListener("touchcancel", reset);
    element.style.touchAction = previousTouchAction;
    element.style.overscrollBehavior = previousOverscrollBehavior;
  };
}
