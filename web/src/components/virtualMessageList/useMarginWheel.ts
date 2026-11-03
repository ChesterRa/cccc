import { useEffect, type RefObject } from "react";

// How far one line of a line-mode wheel (Firefox's) moves the conversation.
const LINE_PX = 20;
const DELTA_LINE = 1;
const DELTA_PAGE = 2;

/**
 * On a wide screen the conversation scrolls in a box as wide as its reading column, so its
 * scrollbar sits beside the messages (lcd.css); the blank margins around that box, and a control
 * floating over them, still belong to the conversation. A wheel there is handed to the log first,
 * so the log's own handlers count it as the reader's scroll, as they would a wheel over a message,
 * and the log then moves by the same distance. A zoom gesture (Ctrl with the wheel or a pinch), a
 * sideways or Shift wheel and a wheel a handler has already taken are left alone. The wheel handed
 * to the log comes back here with the log as its target and is not handed on again.
 */
export function useMarginWheel(logRef: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const onWheel = (event: WheelEvent) => {
      const log = logRef.current;
      const area = log?.parentElement;
      const target = event.target;
      if (!log || !area || !(target instanceof Node)) return;
      if (log.contains(target) || !area.contains(target)) return;
      if (event.defaultPrevented || event.ctrlKey || event.shiftKey) return;
      if (!event.deltaY || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;
      log.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: event.deltaY,
          deltaMode: event.deltaMode,
          bubbles: true,
          cancelable: true,
        }),
      );
      const unit =
        event.deltaMode === DELTA_LINE
          ? LINE_PX
          : event.deltaMode === DELTA_PAGE
            ? log.clientHeight
            : 1;
      log.scrollTop += event.deltaY * unit;
    };
    document.addEventListener("wheel", onWheel, { passive: true });
    return () => document.removeEventListener("wheel", onWheel);
  }, [logRef]);
}
