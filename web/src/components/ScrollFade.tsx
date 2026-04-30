import { useCallback, useRef, useEffect, useState, type ReactNode, type CSSProperties } from "react";
import { classNames } from "../utils/classNames";

interface ScrollFadeProps {
  children: ReactNode;
  className?: string;
  /** Extra classes for the inner scrollable div */
  innerClassName?: string;
  /** Fade width in px (default 24) */
  fadeWidth?: number;
  /** Direction: horizontal (default) or vertical */
  direction?: "horizontal" | "vertical";
  style?: CSSProperties;
}

const INTERACTIVE_SELECTOR = [
  "button",
  "a",
  "input",
  "select",
  "textarea",
  "[role='button']",
  "[role='link']",
  "[role='menuitem']",
  "[role='option']",
  "[role='tab']",
  "[data-scrollfade-no-drag]",
].join(", ");

/**
 * Wrapper that adds gradient fade masks on edges when content overflows.
 * Solves the "scrollbar-hide with no affordance" pattern used across the app.
 */
export function ScrollFade({
  children,
  className,
  innerClassName,
  fadeWidth = 24,
  direction = "horizontal",
  style,
}: ScrollFadeProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [canScrollStart, setCanScrollStart] = useState(false);
  const [canScrollEnd, setCanScrollEnd] = useState(false);
  const dragStateRef = useRef<{ pointerId: number; startX: number; startY: number; startLeft: number; startTop: number; moved: boolean } | null>(null);
  const suppressClickRef = useRef(false);

  const check = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (direction === "horizontal") {
      const tol = 2;
      setCanScrollStart(el.scrollLeft > tol);
      setCanScrollEnd(el.scrollLeft + el.clientWidth < el.scrollWidth - tol);
    } else {
      const tol = 2;
      setCanScrollStart(el.scrollTop > tol);
      setCanScrollEnd(el.scrollTop + el.clientHeight < el.scrollHeight - tol);
    }
  }, [direction]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    check();
    el.addEventListener("scroll", check, { passive: true });

    let ro: ResizeObserver | null = null;
    let mo: MutationObserver | null = null;
    const onWindowResize = () => check();

    const observeWithResizeObserver = () => {
      if (typeof ResizeObserver === "undefined") return;
      ro = new ResizeObserver(check);
      ro.observe(el);
      for (const child of Array.from(el.children)) {
        ro.observe(child);
      }
    };

    observeWithResizeObserver();
    if (!ro) {
      window.addEventListener("resize", onWindowResize);
    }

    if (typeof MutationObserver !== "undefined") {
      mo = new MutationObserver(() => {
        check();
        if (ro) {
          ro.disconnect();
          observeWithResizeObserver();
        }
      });
      mo.observe(el, { childList: true });
    }

    return () => {
      el.removeEventListener("scroll", check);
      ro?.disconnect();
      mo?.disconnect();
      window.removeEventListener("resize", onWindowResize);
    };
  }, [check]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el || direction !== "horizontal") return;

    const onWheel = (event: WheelEvent) => {
      if (el.scrollWidth <= el.clientWidth) return;
      const absX = Math.abs(event.deltaX);
      const absY = Math.abs(event.deltaY);
      if (absY <= absX && !event.shiftKey) return;
      event.preventDefault();
      el.scrollLeft += event.shiftKey && absX > 0 ? event.deltaX : event.deltaY;
      check();
    };

    const onPointerDown = (event: PointerEvent) => {
      if (event.pointerType === "mouse" && event.button !== 0) return;
      if (el.scrollWidth <= el.clientWidth) return;
      const target = event.target;
      if (target instanceof Element && target.closest(INTERACTIVE_SELECTOR)) return;
      dragStateRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        startLeft: el.scrollLeft,
        startTop: el.scrollTop,
        moved: false,
      };
      try {
        el.setPointerCapture(event.pointerId);
      } catch {
        // ignore
      }
    };

    const onPointerMove = (event: PointerEvent) => {
      const drag = dragStateRef.current;
      if (!drag || drag.pointerId !== event.pointerId) return;
      const dx = event.clientX - drag.startX;
      const dy = event.clientY - drag.startY;
      if (!drag.moved && Math.abs(dx) < 3 && Math.abs(dy) < 3) return;
      drag.moved = true;
      el.scrollLeft = drag.startLeft - dx;
      el.scrollTop = drag.startTop - dy;
      check();
      event.preventDefault();
    };

    const finishDrag = (pointerId: number) => {
      const drag = dragStateRef.current;
      if (!drag || drag.pointerId !== pointerId) return;
      suppressClickRef.current = drag.moved;
      dragStateRef.current = null;
      try {
        el.releasePointerCapture(pointerId);
      } catch {
        // ignore
      }
    };

    const onPointerUp = (event: PointerEvent) => {
      finishDrag(event.pointerId);
    };

    const onPointerCancel = (event: PointerEvent) => {
      finishDrag(event.pointerId);
    };

    const onClickCapture = (event: MouseEvent) => {
      if (suppressClickRef.current) {
        suppressClickRef.current = false;
        event.preventDefault();
        event.stopPropagation();
      }
    };

    el.addEventListener("wheel", onWheel, { passive: false });
    el.addEventListener("pointerdown", onPointerDown);
    el.addEventListener("pointermove", onPointerMove);
    el.addEventListener("pointerup", onPointerUp);
    el.addEventListener("pointercancel", onPointerCancel);
    el.addEventListener("click", onClickCapture, true);

    return () => {
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("pointerdown", onPointerDown);
      el.removeEventListener("pointermove", onPointerMove);
      el.removeEventListener("pointerup", onPointerUp);
      el.removeEventListener("pointercancel", onPointerCancel);
      el.removeEventListener("click", onClickCapture, true);
      dragStateRef.current = null;
      suppressClickRef.current = false;
    };
  }, [check, direction]);

  const isH = direction === "horizontal";
  const maskParts: string[] = [];
  if (canScrollStart) {
    maskParts.push(
      isH
        ? `linear-gradient(to right, transparent, black ${fadeWidth}px)`
        : `linear-gradient(to bottom, transparent, black ${fadeWidth}px)`
    );
  }
  if (canScrollEnd) {
    maskParts.push(
      isH
        ? `linear-gradient(to left, transparent, black ${fadeWidth}px)`
        : `linear-gradient(to top, transparent, black ${fadeWidth}px)`
    );
  }

  const maskStyle: CSSProperties =
    maskParts.length === 2
      ? {
          WebkitMaskImage: maskParts.join(", "),
          maskImage: maskParts.join(", "),
          WebkitMaskComposite: "destination-in",
          maskComposite: "intersect",
        }
      : maskParts.length === 1
        ? {
            WebkitMaskImage: maskParts[0],
            maskImage: maskParts[0],
          }
        : {};

  return (
    <div className={classNames("relative", className)} style={{ ...style, ...maskStyle }}>
      <div
        ref={scrollRef}
        className={classNames(
          isH ? "overflow-x-auto scrollbar-hide touch-pan-x" : "overflow-y-auto scrollbar-hide",
          innerClassName
        )}
      >
        {children}
      </div>
    </div>
  );
}
