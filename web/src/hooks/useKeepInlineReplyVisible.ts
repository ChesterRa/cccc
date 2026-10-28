import { useEffect, type RefObject } from "react";

/** Keep the focused inline editor visible as the mobile viewport changes. */
export function useKeepInlineReplyVisible(
  scrollRef: RefObject<HTMLDivElement | null>,
  enabled: boolean,
  topInset: number,
) {
  useEffect(() => {
    if (!enabled) return;
    let frame = 0;
    const adjust = () => {
      frame = 0;
      const scroll = scrollRef.current;
      const editor = document.activeElement;
      if (
        !scroll ||
        !(editor instanceof HTMLTextAreaElement) ||
        !editor.closest("[data-pending-reply]") ||
        !scroll.contains(editor)
      )
        return;
      const viewport = window.visualViewport;
      const area = scroll.getBoundingClientRect();
      const input = editor.getBoundingClientRect();
      const top = Math.max(area.top + topInset, viewport?.offsetTop ?? 0) + 8;
      const bottom =
        Math.min(
          area.bottom,
          (viewport?.offsetTop ?? 0) + (viewport?.height ?? window.innerHeight),
        ) - 8;
      if (bottom <= top) return;
      if (input.height >= bottom - top || input.top < top) scroll.scrollTop += input.top - top;
      else if (input.bottom > bottom) scroll.scrollTop += input.bottom - bottom;
    };
    const schedule = () => {
      if (frame) window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(adjust);
    };
    const viewport = window.visualViewport;
    schedule();
    viewport?.addEventListener("resize", schedule);
    viewport?.addEventListener("scroll", schedule);
    window.addEventListener("resize", schedule);
    return () => {
      if (frame) window.cancelAnimationFrame(frame);
      viewport?.removeEventListener("resize", schedule);
      viewport?.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, [enabled, scrollRef, topInset]);
}
