import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Maximize, Minus, Plus } from "lucide-react";
import { LcdIcon } from "../Icons";
import { buttonVariants } from "../ui/button-variants";

// Only static graphics belong here. PDF and interactive browser surfaces own their input.
type Graphic =
  | { src: string; alt: string; resourceKey?: string; isSvg?: boolean; onBlankClick?: () => void }
  | { svg: string; width: number; height: number; alt: string; onBlankClick?: () => void };
type View = { width: number; height: number };
type Point = { x: number; y: number };

function fitGraphic(content: View, viewport: View, vector: boolean): number {
  if (!content.width || !content.height || !viewport.width || !viewport.height) return 1;
  return Math.min(
    vector ? 8 : 1,
    Math.max(1, viewport.width - 32) / content.width,
    Math.max(1, viewport.height - 32) / content.height,
  );
}

export function GraphicViewer(props: Graphic) {
  // A refreshed request URL can still represent the same image being inspected.
  const key = "src" in props ? (props.resourceKey ?? props.src) : props.svg;
  return <GraphicViewport key={key} {...props} />;
}

function GraphicViewport(graphic: Graphic) {
  const { t } = useTranslation(["chat", "common"]);
  const viewport = useRef<HTMLDivElement>(null);
  const [content, setContent] = useState<View>(
    "svg" in graphic ? { width: graphic.width, height: graphic.height } : { width: 0, height: 0 },
  );
  const [bounds, setBounds] = useState<View>({ width: 0, height: 0 });
  const [manualScale, setManualScale] = useState<number | null>(null);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const failed = "src" in graphic && failedSrc === graphic.src;
  const fitted = fitGraphic(content, bounds, "svg" in graphic || !!graphic.isSvg);
  const scale = manualScale ?? fitted;
  const hasDimensions =
    content.width > 0 && content.height > 0 && bounds.width > 0 && bounds.height > 0;
  const ready = hasDimensions && !failed;
  // A failed refresh must not collapse the canvas and clamp native scroll offsets.
  const width = hasDimensions ? content.width * scale : 0,
    height = hasDimensions ? content.height * scale : 0;
  const stageWidth = Math.max(bounds.width, width + 32),
    stageHeight = Math.max(bounds.height, height + 32);
  const left = (stageWidth - width) / 2,
    top = (stageHeight - height) / 2;
  const anchor = useRef<{ content: Point; screen: Point } | null>(null);
  const pointers = useRef(new Map<number, Point>());
  const gesture = useRef<{
    pointerId: number;
    pointerType: string;
    start: Point;
    image: boolean;
    moved: boolean;
  } | null>(null);
  const lastTap = useRef<{ at: number; point: Point } | null>(null);
  // The synthesized click follows pointerup on touch devices. Keep the overlay mounted until
  // it can consume that click, otherwise the click can activate whatever lies underneath.
  const blankClickPending = useRef(false);
  const pendingWheelScale = useRef<number | null>(null);
  const pinch = useRef<{ distance: number; scale: number; content: Point } | null>(null);
  const geometry = useRef({ scale, fitted, left, top });
  geometry.current = { scale, fitted, left, top };

  useEffect(() => {
    const node = viewport.current;
    if (!node) return;
    const observer = new ResizeObserver(([entry]) => {
      // Use the actual content box; clientWidth/clientHeight round fractional sizes.
      const { width, height } = entry.contentRect;
      setBounds((previous) =>
        previous.width === width && previous.height === height ? previous : { width, height },
      );
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const zoom = (next: number, screen?: Point, previousScreen = screen, restoreCurrent = false) => {
    const node = viewport.current;
    if (!node || !ready) return;
    const current = geometry.current;
    const at = screen || { x: node.clientWidth / 2, y: node.clientHeight / 2 };
    const from = previousScreen || at;
    const bounded = Math.max(Math.min(current.fitted, 0.1), Math.min(8, next));
    if (Math.abs(bounded - current.scale) < 1e-12) {
      anchor.current = null;
      node.scrollLeft += from.x - at.x;
      node.scrollTop += from.y - at.y;
      // A later event may cancel an earlier state update that has not rendered yet.
      if (restoreCurrent) setManualScale(manualScale);
      return;
    }
    anchor.current = {
      content: {
        x: (node.scrollLeft + from.x - current.left) / current.scale,
        y: (node.scrollTop + from.y - current.top) / current.scale,
      },
      screen: at,
    };
    setManualScale(bounded);
  };
  const reset = () => {
    anchor.current = null;
    setManualScale(null);
    viewport.current?.scrollTo(0, 0);
  };

  // Trackpad pinches arrive as ctrl+wheel on desktop browsers. React's delegated
  // wheel listener can be passive, so register this one on the viewport itself.
  const wheelZoom = useRef(zoom);
  wheelZoom.current = zoom;
  const wheelReady = useRef(ready);
  wheelReady.current = ready;
  useEffect(() => {
    const node = viewport.current;
    if (!node) return;
    const onWheel = (event: WheelEvent) => {
      if (!(event.ctrlKey || event.metaKey) || !wheelReady.current) return;
      event.preventDefault();
      const rect = node.getBoundingClientRect();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? node.clientHeight : 1;
      const delta = Math.max(-500, Math.min(500, event.deltaY * unit));
      const current = geometry.current;
      // Several wheel events can arrive before React commits the previous one.
      // Accumulate the requested scale while anchoring against the rendered image.
      const previous = pendingWheelScale.current;
      const next = Math.max(
        Math.min(current.fitted, 0.1),
        Math.min(8, (previous ?? current.scale) * Math.exp(-delta * 0.002)),
      );
      pendingWheelScale.current = next;
      lastTap.current = null;
      const point = { x: event.clientX - rect.left, y: event.clientY - rect.top };
      wheelZoom.current(next, point, point, previous !== null);
    };
    node.addEventListener("wheel", onWheel, { passive: false });
    return () => node.removeEventListener("wheel", onWheel);
  }, []);

  const toggleAt = (point: Point) => {
    const current = geometry.current;
    if (Math.abs(current.scale - current.fitted) < 0.001) zoom(1, point);
    else reset();
  };

  useLayoutEffect(() => {
    const node = viewport.current;
    if (!node) return;
    pendingWheelScale.current = null;
    if (anchor.current) {
      const { content: point, screen } = anchor.current;
      node.scrollLeft = left + point.x * scale - screen.x;
      node.scrollTop = top + point.y * scale - screen.y;
      anchor.current = null;
    } else if (manualScale === null) {
      node.scrollLeft = 0;
      node.scrollTop = 0;
    }
  }, [scale, left, top, manualScale]);

  const button = `${buttonVariants({ variant: "ghost", size: "sm" })} min-w-9 max-sm:min-h-11 max-sm:min-w-11`;
  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
      data-graphic-viewer
    >
      <div
        className="flex shrink-0 flex-wrap items-center justify-center gap-x-3 gap-y-1 border-b border-[var(--glass-border-subtle)] p-1 text-[var(--color-text-secondary)]"
        role="group"
        aria-label={t("graphicViewer.controls")}
        data-graphic-controls
      >
        <div className="flex items-center gap-1">
          <button
            type="button"
            data-lcd-key
            className={button}
            disabled={!ready}
            onClick={reset}
            title={t("graphicViewer.fit")}
            aria-label={t("graphicViewer.fit")}
          >
            <LcdIcon icon={Maximize} glyph="maximize" size={14} data-graphic-fit-icon />
            <span>{t("graphicViewer.fit")}</span>
          </button>
          <button
            type="button"
            data-lcd-key
            className={button}
            disabled={!ready}
            onClick={() => zoom(1)}
            title={t("graphicViewer.actual")}
            aria-label={t("graphicViewer.actual")}
          >
            100%
          </button>
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            data-lcd-key
            className={button}
            disabled={!ready}
            onClick={() => zoom(scale / 1.25)}
            title={t("graphicViewer.out")}
            aria-label={t("graphicViewer.out")}
          >
            <LcdIcon icon={Minus} glyph="minus" size={15} />
          </button>
          <span
            className="min-w-12 text-center text-xs tabular-nums"
            aria-live="off"
            data-graphic-percent
          >
            {ready ? `${Math.round(scale * 100)}%` : "—"}
          </span>
          <button
            type="button"
            data-lcd-key
            className={button}
            disabled={!ready}
            onClick={() => zoom(scale * 1.25)}
            title={t("graphicViewer.in")}
            aria-label={t("graphicViewer.in")}
          >
            <LcdIcon icon={Plus} glyph="plus" size={15} />
          </button>
        </div>
      </div>
      <div className="relative min-h-0 flex-1" data-graphic-canvas>
        <div
          ref={viewport}
          className="relative h-full min-h-0 min-w-0 cursor-grab overflow-auto overscroll-contain bg-[var(--color-bg-primary)] outline-offset-[-2px] active:cursor-grabbing"
          style={{ touchAction: "none", cursor: ready ? undefined : "default" }}
          tabIndex={0}
          role="region"
          aria-label={t("graphicViewer.region", { name: graphic.alt })}
          title={t("graphicViewer.hint")}
          onKeyDown={(event) => {
            if (
              event.target !== event.currentTarget ||
              event.ctrlKey ||
              event.metaKey ||
              event.altKey
            )
              return;
            if (["+", "=", "-", "0", "1"].includes(event.key)) {
              event.preventDefault();
              event.stopPropagation();
              if (event.key === "0") reset();
              else zoom(event.key === "1" ? 1 : scale * (event.key === "-" ? 0.8 : 1.25));
            }
          }}
          onPointerDown={(event) => {
            blankClickPending.current = false;
            if (
              event.button !== 0 ||
              (!ready && !graphic.onBlankClick) ||
              (event.target instanceof Element && event.target.closest("a"))
            )
              return;
            // Keep scrollbar interaction native; pointer capture only begins over the canvas.
            const rect = event.currentTarget.getBoundingClientRect();
            if (
              event.clientX - rect.left >= event.currentTarget.clientWidth ||
              event.clientY - rect.top >= event.currentTarget.clientHeight
            )
              return;
            event.preventDefault();
            event.stopPropagation();
            event.currentTarget.focus({ preventScroll: true });
            event.currentTarget.setPointerCapture(event.pointerId);
            if (pointers.current.size === 0) {
              gesture.current = {
                pointerId: event.pointerId,
                pointerType: event.pointerType,
                start: { x: event.clientX, y: event.clientY },
                image:
                  event.target instanceof Element && !!event.target.closest("[data-graphic-image]"),
                moved: false,
              };
            } else {
              // A pinch is never a click, including after one finger lifts.
              gesture.current = null;
              lastTap.current = null;
            }
            pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
            if (pointers.current.size === 2 && ready) {
              const [first, second] = [...pointers.current.values()];
              const rect = event.currentTarget.getBoundingClientRect();
              const middle = {
                x: (first.x + second.x) / 2 - rect.left,
                y: (first.y + second.y) / 2 - rect.top,
              };
              const current = geometry.current;
              pinch.current = {
                distance: Math.hypot(first.x - second.x, first.y - second.y),
                scale: current.scale,
                content: {
                  x: (event.currentTarget.scrollLeft + middle.x - current.left) / current.scale,
                  y: (event.currentTarget.scrollTop + middle.y - current.top) / current.scale,
                },
              };
            }
          }}
          onPointerMove={(event) => {
            const previous = pointers.current.get(event.pointerId);
            if (!previous) return;
            const active = gesture.current;
            if (
              active?.pointerId === event.pointerId &&
              Math.hypot(event.clientX - active.start.x, event.clientY - active.start.y) > 8
            ) {
              active.moved = true;
              lastTap.current = null;
            }
            const node = event.currentTarget;
            pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
            const after = [...pointers.current.values()];
            if (!ready) return;
            if (after.length === 2 && pinch.current?.distance) {
              const initial = pinch.current;
              const rect = node.getBoundingClientRect();
              const middle = {
                x: (after[0].x + after[1].x) / 2 - rect.left,
                y: (after[0].y + after[1].y) / 2 - rect.top,
              };
              const distance = Math.hypot(after[0].x - after[1].x, after[0].y - after[1].y);
              const current = geometry.current;
              const next = Math.max(
                Math.min(current.fitted, 0.1),
                Math.min(8, (initial.scale * distance) / initial.distance),
              );
              if (Math.abs(next - current.scale) < 1e-12) {
                anchor.current = null;
                node.scrollLeft = current.left + initial.content.x * current.scale - middle.x;
                node.scrollTop = current.top + initial.content.y * current.scale - middle.y;
                setManualScale(manualScale);
              } else {
                anchor.current = { content: initial.content, screen: middle };
                setManualScale(next);
              }
            } else if (after.length === 1) {
              node.scrollLeft -= event.clientX - previous.x;
              node.scrollTop -= event.clientY - previous.y;
            }
          }}
          onPointerUp={(event) => {
            pointers.current.delete(event.pointerId);
            if (pointers.current.size < 2) pinch.current = null;
            const active = gesture.current;
            if (!active || active.pointerId !== event.pointerId) return;
            gesture.current = null;
            if (active.moved || pointers.current.size) return;
            if (!active.image) {
              lastTap.current = null;
              blankClickPending.current = !!graphic.onBlankClick;
              return;
            }
            if (!ready) return;
            const rect = event.currentTarget.getBoundingClientRect();
            const point = { x: event.clientX - rect.left, y: event.clientY - rect.top };
            if (active.pointerType === "touch") {
              const previous = lastTap.current;
              const now = Date.now();
              if (
                previous &&
                now - previous.at <= 350 &&
                Math.hypot(point.x - previous.point.x, point.y - previous.point.y) <= 32
              ) {
                lastTap.current = null;
                toggleAt(point);
              } else {
                lastTap.current = { at: now, point };
              }
            } else {
              lastTap.current = null;
              toggleAt(point);
            }
          }}
          onClick={(event) => {
            if (!blankClickPending.current) return;
            blankClickPending.current = false;
            event.preventDefault();
            event.stopPropagation();
            graphic.onBlankClick?.();
          }}
          onPointerCancel={(event) => {
            blankClickPending.current = false;
            pointers.current.delete(event.pointerId);
            pinch.current = null;
            gesture.current = null;
            lastTap.current = null;
          }}
          onLostPointerCapture={(event) => {
            pointers.current.delete(event.pointerId);
            if (pointers.current.size < 2) pinch.current = null;
            if (gesture.current?.pointerId === event.pointerId) gesture.current = null;
          }}
        >
          <div
            // CSS owns the viewport minimum. A measured minimum would feed scrollbar
            // changes back into overflow and repeat the resize/fit cycle.
            style={{
              position: "relative",
              minWidth: "100%",
              minHeight: "100%",
              width: width + 32,
              height: height + 32,
            }}
          >
            <div
              className="select-none [&_svg]:!m-0 [&_svg]:!h-full [&_svg]:!w-full [&_svg]:!max-w-none"
              data-graphic-image
              style={{
                position: "absolute",
                left,
                top,
                width,
                height,
                visibility: ready ? "visible" : "hidden",
              }}
            >
              {"src" in graphic ? (
                <img
                  src={graphic.src}
                  alt={graphic.alt}
                  draggable={false}
                  style={{ width: "100%", height: "100%", maxWidth: "none", objectFit: "contain" }}
                  onLoad={(event) =>
                    setContent({
                      width: event.currentTarget.naturalWidth,
                      height: event.currentTarget.naturalHeight,
                    })
                  }
                  onError={() => setFailedSrc(graphic.src)}
                />
              ) : (
                <div className="h-full w-full" dangerouslySetInnerHTML={{ __html: graphic.svg }} />
              )}
            </div>
            {!hasDimensions && !failed && (
              <p role="status" className="p-4 text-sm text-[var(--color-text-muted)]">
                {t("common:loading")}
              </p>
            )}
          </div>
        </div>
        {failed && (
          <div className="pointer-events-none absolute inset-0 bg-[var(--color-bg-primary)] p-4">
            <p role="alert" className="text-sm">
              {t("imagePreviewUnavailable")}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
