import { useRef, type PointerEvent } from "react";
import { shouldHandleSwipeNavigation } from "../../hooks/useSwipeNavigation";

export type TerminalPageAction = (direction: -1 | 1) => void;

// Bind only to the compact title bar. The terminal owns all gestures in its body.
export function useTerminalTitlePaging(onPage?: TerminalPageAction) {
  const start = useRef<{ id: number; x: number; y: number } | null>(null);
  const clear = () => {
    start.current = null;
  };
  return {
    onPointerDown(event: PointerEvent<HTMLDivElement>) {
      clear();
      if (
        !onPage ||
        event.pointerType !== "touch" ||
        !event.isPrimary ||
        !shouldHandleSwipeNavigation(event.target, event.currentTarget)
      )
        return;
      start.current = { id: event.pointerId, x: event.clientX, y: event.clientY };
    },
    onPointerMove(event: PointerEvent<HTMLDivElement>) {
      if (start.current?.id !== event.pointerId) return;
      // A vertical gesture belongs to scrolling, even if it later turns sideways.
      if (Math.abs(event.clientY - start.current.y) > 16) clear();
    },
    onPointerUp(event: PointerEvent<HTMLDivElement>) {
      const origin = start.current;
      clear();
      if (
        !origin ||
        origin.id !== event.pointerId ||
        !onPage ||
        window.getSelection()?.isCollapsed === false
      )
        return;
      const dx = event.clientX - origin.x,
        dy = event.clientY - origin.y;
      if (Math.abs(dx) < 60 || Math.abs(dx) <= Math.abs(dy) * 2) return;
      event.stopPropagation();
      onPage(dx < 0 ? 1 : -1);
    },
    onPointerCancel: clear,
    onLostPointerCapture: clear,
  };
}
