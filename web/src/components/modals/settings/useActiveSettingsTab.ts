import { useLayoutEffect, useState } from "react";

/** Reveal the active mobile section without scrolling the settings form or page. */
export function useActiveSettingsTab() {
  const [tab, setTab] = useState<HTMLButtonElement | null>(null);

  useLayoutEffect(() => {
    const strip = tab?.parentElement;
    if (!tab || !strip) return;
    const reveal = () => {
      if (strip.clientWidth === 0) return;
      const item = tab.getBoundingClientRect();
      const viewport = strip.getBoundingClientRect();
      // Keep the active label clear of the strip's edge fade.
      const inset = 24;
      if (item.left < viewport.left + inset) strip.scrollLeft -= viewport.left + inset - item.left;
      else if (item.right > viewport.right - inset)
        strip.scrollLeft += item.right - viewport.right + inset;
    };
    reveal();
    const observer = new ResizeObserver(reveal);
    observer.observe(strip);
    observer.observe(tab);
    return () => observer.disconnect();
  }, [tab]);

  return setTab;
}
