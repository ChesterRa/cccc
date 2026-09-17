import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Maximize, Minus, Plus } from "lucide-react";
import { buttonVariants } from "../ui/button-variants";

// Only static graphics belong here. PDF and interactive browser surfaces own their input.
type Graphic =
  | { src: string; alt: string; resourceKey?: string }
  | { svg: string; width: number; height: number; alt: string };
type View = { width: number; height: number };
type Point = { x: number; y: number };

function fitGraphic(content: View, viewport: View): number {
  if (!content.width || !content.height || !viewport.width || !viewport.height) return 1;
  return Math.min(
    1,
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
  const fitted = fitGraphic(content, bounds);
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

  const zoom = (next: number, screen?: Point, previousScreen = screen) => {
    const node = viewport.current;
    if (!node || !ready) return;
    const current = geometry.current;
    const at = screen || { x: node.clientWidth / 2, y: node.clientHeight / 2 };
    const from = previousScreen || at;
    const bounded = Math.max(Math.min(current.fitted, 0.1), Math.min(8, next));
    if (bounded === current.scale) {
      anchor.current = null;
      node.scrollLeft += from.x - at.x;
      node.scrollTop += from.y - at.y;
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

  useLayoutEffect(() => {
    const node = viewport.current;
    if (!node) return;
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
      >
        <div className="flex items-center gap-1">
          <button
            type="button"
            className={button}
            disabled={!ready}
            onClick={reset}
            title={t("graphicViewer.fit")}
            aria-label={t("graphicViewer.fit")}
          >
            <Maximize size={14} />
            <span>{t("graphicViewer.fit")}</span>
          </button>
          <button
            type="button"
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
            className={button}
            disabled={!ready}
            onClick={() => zoom(scale / 1.25)}
            title={t("graphicViewer.out")}
            aria-label={t("graphicViewer.out")}
          >
            <Minus size={15} />
          </button>
          <span className="min-w-12 text-center text-xs tabular-nums" aria-live="off">
            {ready ? `${Math.round(scale * 100)}%` : "—"}
          </span>
          <button
            type="button"
            className={button}
            disabled={!ready}
            onClick={() => zoom(scale * 1.25)}
            title={t("graphicViewer.in")}
            aria-label={t("graphicViewer.in")}
          >
            <Plus size={15} />
          </button>
        </div>
      </div>
      <div className="relative min-h-0 flex-1">
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
            if (
              event.button !== 0 ||
              !ready ||
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
            pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
          }}
          onPointerMove={(event) => {
            const previous = pointers.current.get(event.pointerId);
            if (!previous) return;
            const node = event.currentTarget;
            const before = [...pointers.current.values()];
            pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
            const after = [...pointers.current.values()];
            if (after.length === 2) {
              const distance = (points: Point[]) =>
                Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y);
              const middle = (points: Point[]) => {
                const rect = node.getBoundingClientRect();
                return {
                  x: (points[0].x + points[1].x) / 2 - rect.left,
                  y: (points[0].y + points[1].y) / 2 - rect.top,
                };
              };
              if (distance(before) > 0)
                zoom(
                  (geometry.current.scale * distance(after)) / distance(before),
                  middle(after),
                  middle(before),
                );
            } else if (after.length === 1) {
              node.scrollLeft -= event.clientX - previous.x;
              node.scrollTop -= event.clientY - previous.y;
            }
          }}
          onPointerUp={(event) => {
            pointers.current.delete(event.pointerId);
          }}
          onPointerCancel={(event) => {
            pointers.current.delete(event.pointerId);
          }}
          onLostPointerCapture={(event) => {
            pointers.current.delete(event.pointerId);
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
