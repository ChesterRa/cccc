// useViewportHeight: Handles the virtual keyboard on mobile.
//
// Problem: mobile browsers disagree on whether `100dvh` includes the virtual
// keyboard. Subtracting the keyboard offset from `100dvh` can therefore shrink
// the app twice on iOS.
//
// Solution: use the visual viewport height directly when it is available.
import { useEffect } from "react";

export function getVisualViewportHeight(height: number): string | null {
  return Number.isFinite(height) && height > 0 ? `${height}px` : null;
}

export function useViewportHeight() {
  useEffect(() => {
    const vv = window.visualViewport;
    if (!vv) return; // Not supported (desktop browsers, older mobile)

    function syncHeight() {
      if (!vv) return;
      const height = getVisualViewportHeight(vv.height);
      if (height) document.documentElement.style.setProperty("--app-viewport-height", height);
    }

    vv.addEventListener("resize", syncHeight);
    syncHeight();

    return () => {
      vv.removeEventListener("resize", syncHeight);
      document.documentElement.style.removeProperty("--app-viewport-height");
    };
  }, []);
}
