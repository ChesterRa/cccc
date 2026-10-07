import type { CSSProperties } from "react";
import { VISUAL_VIEWPORT_BOX } from "../../hooks/useViewportHeight";

/**
 * A fixed overlay sizes against the layout viewport, which the iOS keyboard
 * covers; follow the visible viewport instead so inputs (e.g. a terminal's
 * prompt row) stay above the keyboard. Inline frames keep their host layout.
 */
export function getModalOverlayStyle({
  inline,
  isOpen,
}: {
  inline: boolean;
  isOpen: boolean;
}): CSSProperties | undefined {
  if (inline) return undefined;
  return {
    ...VISUAL_VIEWPORT_BOX,
    bottom: "auto",
    ...(isOpen ? { backdropFilter: "blur(12px)", WebkitBackdropFilter: "blur(12px)" } : {}),
  };
}
