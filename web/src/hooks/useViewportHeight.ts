// useViewportHeight: Handles the virtual keyboard on mobile.
//
// Problem: mobile browsers disagree on whether `100dvh` includes the virtual
// keyboard. Subtracting the keyboard offset from `100dvh` can therefore shrink
// the app twice on iOS.
//
// Solution: size and position the app from the visual viewport. iOS may pan the
// visual viewport to reveal a focused textarea, so height alone is insufficient:
// the app must also follow the visual viewport's top edge.
import { useEffect } from "react";

export interface VisualViewportLayout {
  height: string;
  offsetTop: string;
}

export function getVisualViewportLayout(
  height: number,
  offsetTop: number,
  pageTop: number,
  windowScrollY: number,
): VisualViewportLayout | null {
  if (!Number.isFinite(height) || height <= 0) return null;

  // pageTop - scrollY is equivalent to offsetTop in conforming browsers, but
  // remains useful when WebKit briefly reports a stale zero offset during the
  // keyboard animation.
  const pageOffset =
    Number.isFinite(pageTop) && Number.isFinite(windowScrollY) ? pageTop - windowScrollY : 0;
  const top = Math.max(0, Number.isFinite(offsetTop) ? offsetTop : 0, pageOffset);
  return { height: `${height}px`, offsetTop: `${top}px` };
}

export function useViewportHeight() {
  useEffect(() => {
    const vv = window.visualViewport;
    if (!vv) return; // Not supported (desktop browsers, older mobile)
    let delayedSync: number | undefined;

    function syncViewport() {
      if (!vv) return;
      const layout = getVisualViewportLayout(vv.height, vv.offsetTop, vv.pageTop, window.scrollY);
      if (!layout) return;
      document.documentElement.style.setProperty("--app-viewport-height", layout.height);
      document.documentElement.style.setProperty("--app-viewport-offset-top", layout.offsetTop);
    }

    function scheduleSync() {
      syncViewport();
      window.clearTimeout(delayedSync);
      // WebKit can expose the final offset a frame after the resize event.
      delayedSync = window.setTimeout(syncViewport, 50);
    }

    vv.addEventListener("resize", scheduleSync);
    vv.addEventListener("scroll", scheduleSync);
    window.addEventListener("resize", scheduleSync);
    window.addEventListener("scroll", scheduleSync);
    scheduleSync();

    return () => {
      vv.removeEventListener("resize", scheduleSync);
      vv.removeEventListener("scroll", scheduleSync);
      window.removeEventListener("resize", scheduleSync);
      window.removeEventListener("scroll", scheduleSync);
      window.clearTimeout(delayedSync);
      document.documentElement.style.removeProperty("--app-viewport-height");
      document.documentElement.style.removeProperty("--app-viewport-offset-top");
    };
  }, []);
}
