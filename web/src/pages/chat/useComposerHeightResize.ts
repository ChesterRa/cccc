import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { useUIStore } from "../../stores/useUIStore";
import {
  clampComposerHeight,
  composerHeightLimit,
  COMPOSER_DEFAULT_HEIGHT,
} from "../../utils/composerHeight";
import { resizeComposerTextarea } from "./useComposerTextareaAutoResize";

export function useComposerHeightResize({
  footerRef,
  composerRef,
  enabled,
  scale,
}: {
  footerRef: RefObject<HTMLElement | null>;
  composerRef: RefObject<HTMLTextAreaElement | null>;
  enabled: boolean;
  scale: number;
}) {
  const preferred = useUIStore((state) => state.composerHeight);
  const save = useUIStore((state) => state.setComposerHeight);
  const [limit, setLimit] = useState(COMPOSER_DEFAULT_HEIGHT);
  const [resizing, setResizing] = useState(false);
  const handleRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<(() => void) | null>(null);
  const limitRef = useRef(limit);
  limitRef.current = limit;
  const height = clampComposerHeight(preferred, limit);
  const minimum = COMPOSER_DEFAULT_HEIGHT * scale;

  useLayoutEffect(() => {
    if (!enabled) return;
    const footer = footerRef.current;
    const panel = footer?.parentElement;
    const textarea = composerRef.current;
    if (!panel || !footer || !textarea) return;
    const messages = panel.querySelector(":scope > main");
    const measure = () => {
      const panelHeight = panel.getBoundingClientRect().height;
      const messageHeight = messages?.getBoundingClientRect().height || 0;
      const chromeHeight = panelHeight - messageHeight - textarea.getBoundingClientRect().height;
      setLimit(composerHeightLimit(panelHeight, chromeHeight, scale));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    observer.observe(footer);
    if (messages) observer.observe(messages);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [enabled, scale, footerRef, composerRef]);

  const preview = useCallback(
    (value: number) => {
      const next = clampComposerHeight(value, limitRef.current);
      const node = composerRef.current;
      if (node) {
        footerRef.current?.style.setProperty("--composer-max-height", `${next * scale}px`);
        resizeComposerTextarea(node, minimum, next * scale);
      }
      handleRef.current?.setAttribute("aria-valuenow", String(Math.round(next * scale)));
      return next;
    },
    [composerRef, footerRef, minimum, scale],
  );

  useLayoutEffect(() => {
    cancelRef.current?.();
    if (enabled) preview(height);
    else footerRef.current?.style.removeProperty("--composer-max-height");
    return () => cancelRef.current?.();
  }, [enabled, height, limit, preview, footerRef]);

  const onPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (!enabled || event.button !== 0 || event.isPrimary === false) return;
      event.preventDefault();
      event.stopPropagation();
      cancelRef.current?.();
      const pointerId = event.pointerId;
      const startY = event.clientY;
      let pending = height;
      const body = document.body;
      const cursor = body.style.cursor;
      const userSelect = body.style.userSelect;
      body.style.cursor = "ns-resize";
      body.style.userSelect = "none";
      setResizing(true);
      const move = (next: globalThis.PointerEvent) => {
        if (next.pointerId !== pointerId) return;
        next.preventDefault();
        pending = preview(height + (startY - next.clientY) / scale);
      };
      const finish = (commit: boolean) => {
        cancelRef.current = null;
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", cancel);
        window.removeEventListener("blur", cancel);
        body.style.cursor = cursor;
        body.style.userSelect = userSelect;
        setResizing(false);
        if (commit) save(clampComposerHeight(pending, limitRef.current));
        else preview(height);
      };
      const up = (next: globalThis.PointerEvent) => {
        if (next.pointerId === pointerId) finish(true);
      };
      const cancel = () => finish(false);
      cancelRef.current = cancel;
      window.addEventListener("pointermove", move, { passive: false });
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", cancel);
      window.addEventListener("blur", cancel);
    },
    [enabled, height, preview, save, scale],
  );

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!enabled) return;
    const next =
      event.key === "ArrowUp"
        ? height + 16
        : event.key === "ArrowDown"
          ? height - 16
          : event.key === "Home"
            ? COMPOSER_DEFAULT_HEIGHT
            : event.key === "End"
              ? limit
              : null;
    if (next === null) return;
    event.preventDefault();
    cancelRef.current?.();
    save(clampComposerHeight(next, limit));
  };
  const onReset = () => {
    if (!enabled) return;
    cancelRef.current?.();
    save(COMPOSER_DEFAULT_HEIGHT);
  };
  return {
    handleRef,
    height: height * scale,
    minimum,
    maximum: limit * scale,
    resizing,
    onPointerDown,
    onKeyDown,
    onReset,
  };
}
