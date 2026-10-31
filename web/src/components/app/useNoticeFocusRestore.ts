import { useLayoutEffect, useRef, type FocusEvent } from "react";

/** Return focus when a notice disappears while one of its controls owns focus. */
export function useNoticeFocusRestore() {
  const rootRef = useRef<HTMLElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    const root = rootRef.current;
    return () => {
      if (!root?.contains(document.activeElement)) return;
      const previous = previousFocusRef.current;
      if (
        previous?.isConnected &&
        previous.getClientRects().length &&
        !previous.closest("[inert]")
      ) {
        previous.focus();
        if (document.activeElement === previous) return;
      }
      const fallback = Array.from(
        document.querySelectorAll<HTMLButtonElement>("main [data-group-header-work] button"),
      ).find((button) => {
        const rect = button.getBoundingClientRect();
        return (
          !button.disabled &&
          !button.closest('[inert], [aria-hidden="true"]') &&
          rect.width > 0 &&
          rect.height > 0 &&
          rect.right > 0 &&
          rect.bottom > 0 &&
          rect.left < window.innerWidth &&
          rect.top < window.innerHeight
        );
      });
      fallback?.focus();
    };
  }, []);

  const onFocusCapture = (event: FocusEvent<HTMLElement>) => {
    const previous = event.relatedTarget;
    if (previous instanceof HTMLElement && !event.currentTarget.contains(previous))
      previousFocusRef.current = previous;
  };
  return { rootRef, onFocusCapture };
}
