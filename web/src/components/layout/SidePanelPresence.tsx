import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

/**
 * Shows the side panel while `open`, and lets the space it held close smoothly once it is
 * gone: the panel's content leaves at once (its state belongs to the surface that closed),
 * and a spacer of its last width collapses in its place where the style animates one. A
 * style without that animation, or reduced motion, drops the spacer at once.
 */
export function SidePanelPresence({ open, children }: { open: boolean; children: ReactNode }) {
  const host = useRef<HTMLDivElement>(null);
  const spacer = useRef<HTMLDivElement>(null);
  const [wasOpen, setWasOpen] = useState(open);
  const [closingWidth, setClosingWidth] = useState(0);
  // Decided while rendering, so the panel and its spacer never both miss a frame. The panel
  // is still in the document at this point, so the width it was laid out to is read from it
  // here, once per close, rather than tracked on every render of the chat. Its laid-out
  // width, not its measured one: while it is still growing open it measures less.
  if (wasOpen !== open) {
    setWasOpen(open);
    const panel = open ? null : host.current?.querySelector<HTMLElement>("#group-side-panel");
    const width = Number.parseFloat(panel?.style.width || "");
    setClosingWidth(Number.isFinite(width) && width > 0 ? width : 0);
  }

  useLayoutEffect(() => {
    if (!closingWidth) return;
    const node = spacer.current;
    const animated = !!node && getComputedStyle(node).animationName !== "none";
    if (!animated) setClosingWidth(0);
  }, [closingWidth]);

  return (
    <div ref={host} className="contents" data-side-panel-presence={open ? "open" : "closed"}>
      {open ? children : null}
      {!open && closingWidth > 0 ? (
        <div
          ref={spacer}
          className="side-panel-exit hidden min-h-0 flex-shrink-0 md:block"
          style={{ width: `${closingWidth}px` }}
          aria-hidden="true"
          data-side-panel-closing
          onAnimationEnd={() => setClosingWidth(0)}
        />
      ) : null}
    </div>
  );
}
