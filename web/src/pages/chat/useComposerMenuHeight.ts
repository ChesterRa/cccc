import { useLayoutEffect, type RefObject } from "react";

import { VISUAL_VIEWPORT_BOX_ATTRIBUTE } from "../../hooks/useViewportHeight";

/**
 * Top edge of the visible viewport in client coordinates. The marked app box
 * follows the visual viewport, and the difference of two client rects cancels
 * whichever origin the browser uses for them (visual or layout viewport; iOS
 * WebKit can report the former). `visualViewport.offsetTop` is only correct
 * for a layout-viewport origin and double-counts the keyboard pan otherwise.
 *
 * Without a marked ancestor (e.g. a menu portaled out of the app) this falls
 * back to 0, which is only right for a visual-viewport origin; mark the
 * surface that follows the visible viewport instead of relying on it.
 */
function getVisibleTop(anchor: Element): number {
  return anchor.closest(`[${VISUAL_VIEWPORT_BOX_ATTRIBUTE}]`)?.getBoundingClientRect().top ?? 0;
}

/** Keep menus above the composer inside the visible viewport, including keyboard panning. */
export function useComposerMenuHeight(ref: RefObject<HTMLDivElement | null>, active = true) {
  useLayoutEffect(() => {
    const menu = ref.current;
    const anchor = menu?.parentElement;
    if (!active || !menu || !anchor) return;
    const viewport = window.visualViewport;
    let timer = 0;

    const measure = () => {
      const gap = Number.parseFloat(getComputedStyle(menu).marginBottom) || 0;
      const available = Math.max(
        0,
        anchor.getBoundingClientRect().top - getVisibleTop(anchor) - gap - 8,
      );
      menu.style.maxHeight = `min(15rem, ${available}px)`;
    };
    const schedule = () => {
      measure();
      window.clearTimeout(timer);
      // The app applies its visual-viewport height during the same resize event.
      timer = window.setTimeout(measure, 50);
    };
    measure();
    const observer = new ResizeObserver(schedule);
    observer.observe(anchor);
    window.addEventListener("resize", schedule);
    window.addEventListener("scroll", schedule, true);
    viewport?.addEventListener("resize", schedule);
    viewport?.addEventListener("scroll", schedule);
    return () => {
      window.clearTimeout(timer);
      observer.disconnect();
      window.removeEventListener("resize", schedule);
      window.removeEventListener("scroll", schedule, true);
      viewport?.removeEventListener("resize", schedule);
      viewport?.removeEventListener("scroll", schedule);
      menu.style.removeProperty("max-height");
    };
  }, [ref, active]);
}
