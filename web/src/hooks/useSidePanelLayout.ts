import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
} from "react";
import { getChatSession, useUIStore } from "../stores/useUIStore";
import { useModalStore } from "../stores/useModalStore";
import {
  clampSidePanelWidth,
  shouldCompactSidePanel,
  SIDE_PANEL_COMPACT_WIDTH,
  SIDE_PANEL_MIN_WIDTH,
} from "../utils/sidePanelLayout";
import type { SidePanelSurface } from "./useSidePanelSelection";

type Layout = { width: number; compact: boolean };

export function useSidePanelLayout(
  groupId: string,
  surface: SidePanelSurface | null,
  viewing: boolean,
  container: RefObject<HTMLDivElement | null>,
) {
  const session = useUIStore((state) => getChatSession(groupId, state.chatSessions));
  const save = useUIStore((state) => state.setChatSidePanelLayout);
  const closeViewer = useModalStore((state) => state.setPresentationViewer);
  const [containerWidth, setContainerWidth] = useState(0);
  const [draft, setDraft] = useState<Layout | null>(null);
  const gesture = useRef<{ x: number; width: number; latest: Layout } | null>(null);
  const compact =
    surface === "presentation" && (draft?.compact ?? (!viewing && session.presentationCompact));
  const expandedWidth = clampSidePanelWidth(draft?.width ?? session.sidePanelWidth, containerWidth);
  const width = compact ? SIDE_PANEL_COMPACT_WIDTH : expandedWidth;
  const dragging = draft !== null;

  useEffect(() => {
    const node = container.current;
    if (!node) return;
    const update = () => setContainerWidth(node.clientWidth);
    update();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    }
    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  }, [container, surface]);

  useEffect(() => {
    if (!dragging) return;
    let frame: number | null = null;
    const oldCursor = document.body.style.cursor;
    const oldSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const move = (event: globalThis.PointerEvent) => {
      const current = gesture.current;
      if (!current) return;
      const proposed = current.width - (event.clientX - current.x);
      const nextCompact =
        surface === "presentation" && shouldCompactSidePanel(proposed, current.latest.compact);
      current.latest = {
        width: nextCompact ? current.latest.width : clampSidePanelWidth(proposed, containerWidth),
        compact: nextCompact,
      };
      if (frame === null) {
        frame = requestAnimationFrame(() => {
          frame = null;
          const next = gesture.current?.latest;
          if (next)
            setDraft((previous) =>
              previous?.width === next.width && previous.compact === next.compact ? previous : next,
            );
        });
      }
    };
    const finish = (event: globalThis.PointerEvent) => {
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      const next = gesture.current?.latest;
      if (next && event.type === "pointerup") {
        save(groupId, {
          width: next.compact ? undefined : next.width,
          ...(surface === "presentation" ? { compact: next.compact } : {}),
        });
        if (next.compact && viewing) closeViewer(null);
      }
      gesture.current = null;
      setDraft(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      if (frame !== null) cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      document.body.style.cursor = oldCursor;
      document.body.style.userSelect = oldSelect;
      gesture.current = null;
      setDraft(null);
    };
  }, [dragging, groupId, surface, viewing, containerWidth, save, closeViewer]);

  const toggleCompact = useCallback(() => {
    if (surface !== "presentation") return;
    save(groupId, { compact: !compact });
    if (!compact && viewing) closeViewer(null);
  }, [surface, groupId, compact, viewing, save, closeViewer]);
  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (!surface || event.button !== 0) return;
    event.preventDefault();
    const latest = { width: expandedWidth, compact };
    gesture.current = { x: event.clientX, width, latest };
    setDraft(latest);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter" && surface === "presentation") {
      event.preventDefault();
      toggleCompact();
    } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      const proposed = width + (event.key === "ArrowLeft" ? 24 : -24);
      const nextCompact =
        surface === "presentation" &&
        (compact
          ? event.key === "ArrowRight"
          : event.key === "ArrowRight" && width <= SIDE_PANEL_MIN_WIDTH);
      save(groupId, {
        width: nextCompact
          ? undefined
          : clampSidePanelWidth(compact ? session.sidePanelWidth : proposed, containerWidth),
        ...(surface === "presentation" ? { compact: nextCompact } : {}),
      });
      if (nextCompact && viewing) closeViewer(null);
    }
  };
  return {
    width,
    compact,
    dragging,
    toggleCompact,
    onPointerDown,
    onKeyDown,
    maxWidth: clampSidePanelWidth(Number.MAX_SAFE_INTEGER, containerWidth || undefined),
  };
}
