import { useLayoutEffect, type RefObject } from "react";

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
        anchor.getBoundingClientRect().top - (viewport?.offsetTop || 0) - gap - 8,
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
