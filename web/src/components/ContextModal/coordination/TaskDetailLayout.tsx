import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { classNames } from "../../../utils/classNames";
import { SIDE_PANEL_DIVIDER_WIDTH } from "../../../utils/sidePanelLayout";
import { PanelResizeHandle } from "../../layout/PanelResizeHandle";

const BOARD_MIN_WIDTH = 600;
const DETAIL_MIN_WIDTH = 440;
const DETAIL_WIDTH_KEY = "cccc-task-detail-width";

function readWidth(): number | null {
  try {
    const width = Number(localStorage.getItem(DETAIL_WIDTH_KEY));
    return Number.isFinite(width) && width > 0 ? width : null;
  } catch {
    return null;
  }
}

/** The task board stays mounted in the same scroll area in either layout. */
export function TaskDetailLayout({
  scrollRef,
  isDark,
  detailLabel,
  resizeLabel,
  renderDetail,
  children,
}: {
  scrollRef: RefObject<HTMLDivElement | null>;
  isDark: boolean;
  detailLabel: string;
  resizeLabel: string;
  renderDetail?: (singlePane: boolean) => ReactNode;
  children: ReactNode;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const detailRef = useRef<HTMLElement>(null);
  const detailId = useId();
  const boardFocus = useRef<HTMLElement | null>(null);
  const wasShowingDetail = useRef(false);
  const drag = useRef<{ id: number; x: number; startWidth: number; width: number } | null>(null);
  const [containerWidth, setContainerWidth] = useState(0);
  const [preferredWidth, setPreferredWidth] = useState(readWidth);
  const [dragWidth, setDragWidth] = useState<number | null>(null);

  const showingDetail = renderDetail !== undefined;
  const maxDetailWidth = containerWidth - BOARD_MIN_WIDTH - SIDE_PANEL_DIVIDER_WIDTH;
  const split = showingDetail && maxDetailWidth >= DETAIL_MIN_WIDTH;
  const singlePane = showingDetail && !split;
  const clampWidth = (width: number) =>
    Math.min(maxDetailWidth, Math.max(DETAIL_MIN_WIDTH, Math.round(width)));
  const width = split ? clampWidth(dragWidth ?? preferredWidth ?? containerWidth * 0.4) : 0;

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const measure = () => setContainerWidth(container.clientWidth);
    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (singlePane && !detailRef.current?.contains(document.activeElement)) {
      detailRef.current?.focus({ preventScroll: true });
    } else if (
      !showingDetail &&
      wasShowingDetail.current &&
      document.activeElement === document.body
    ) {
      const previous = boardFocus.current;
      const target =
        previous && scrollRef.current?.contains(previous) ? previous : scrollRef.current;
      target?.focus({ preventScroll: true });
    }
    wasShowingDetail.current = showingDetail;
  }, [scrollRef, showingDetail, singlePane]);

  useLayoutEffect(() => {
    if (split || !drag.current) return;
    drag.current = null;
    setDragWidth(null);
  }, [split]);

  function rememberWidth(nextWidth: number) {
    setPreferredWidth(nextWidth);
    try {
      localStorage.setItem(DETAIL_WIDTH_KEY, String(nextWidth));
    } catch {
      // The adjusted width still applies while this panel is open.
    }
  }

  function endDrag(event: PointerEvent<HTMLDivElement>, save: boolean) {
    const gesture = drag.current;
    if (!gesture || gesture.id !== event.pointerId) return;
    drag.current = null;
    setDragWidth(null);
    if (save && gesture.width !== gesture.startWidth) rememberWidth(gesture.width);
  }

  return (
    <div
      ref={containerRef}
      data-task-detail-layout={split ? "split" : singlePane ? "detail" : "board"}
      className={classNames(
        "grid min-h-0 min-w-0 flex-1 grid-cols-[minmax(0,1fr)] grid-rows-[minmax(0,1fr)] overflow-hidden",
        dragWidth !== null && "cursor-col-resize select-none",
      )}
      style={
        split
          ? { gridTemplateColumns: `minmax(0,1fr) ${SIDE_PANEL_DIVIDER_WIDTH}px ${width}px` }
          : undefined
      }
    >
      <div
        ref={scrollRef}
        tabIndex={-1}
        data-context-scroll
        inert={singlePane || undefined}
        onFocusCapture={(event) => {
          if (event.target instanceof HTMLElement) boardFocus.current = event.target;
        }}
        className={classNames(
          "col-start-1 row-start-1 min-h-0 min-w-0 overflow-y-auto bg-[var(--color-bg-primary)]",
          singlePane && "invisible",
        )}
      >
        {children}
      </div>
      {split && (
        <PanelResizeHandle
          isDark={isDark}
          dragging={dragWidth !== null}
          className="col-start-2 row-start-1"
          aria-label={resizeLabel}
          aria-controls={detailId}
          aria-valuemin={DETAIL_MIN_WIDTH}
          aria-valuemax={maxDetailWidth}
          aria-valuenow={width}
          onPointerDown={(event) => {
            if (event.button !== 0 || drag.current) return;
            event.preventDefault();
            event.currentTarget.setPointerCapture?.(event.pointerId);
            drag.current = { id: event.pointerId, x: event.clientX, startWidth: width, width };
            setDragWidth(width);
          }}
          onPointerMove={(event) => {
            const gesture = drag.current;
            if (!gesture || gesture.id !== event.pointerId) return;
            gesture.width = clampWidth(gesture.startWidth + gesture.x - event.clientX);
            setDragWidth(gesture.width);
          }}
          onPointerUp={(event) => endDrag(event, true)}
          onPointerCancel={(event) => endDrag(event, false)}
          onLostPointerCapture={(event) => endDrag(event, false)}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            const nextWidth = clampWidth(width + (event.key === "ArrowLeft" ? 24 : -24));
            if (nextWidth !== width) rememberWidth(nextWidth);
          }}
        />
      )}
      {showingDetail && (
        <section
          ref={detailRef}
          id={detailId}
          aria-label={detailLabel}
          tabIndex={-1}
          data-task-detail
          className={classNames(
            "row-start-1 flex min-h-0 min-w-0 flex-col overflow-hidden bg-[var(--color-bg-secondary)]",
            split ? "col-start-3" : "col-start-1",
          )}
        >
          {renderDetail(singlePane)}
        </section>
      )}
    </div>
  );
}
