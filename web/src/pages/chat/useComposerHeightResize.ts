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
  COMPOSER_AUTO_MAX_HEIGHT,
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
  const height =
    preferred === null ? COMPOSER_DEFAULT_HEIGHT : clampComposerHeight(preferred, limit);
  const minimum = COMPOSER_DEFAULT_HEIGHT * scale;
  const minHeight = preferred === null ? minimum : height * scale;
  const maxHeight =
    preferred === null ? Math.min(COMPOSER_AUTO_MAX_HEIGHT, limit) * scale : height * scale;

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
      const textareaHeight = textarea.getBoundingClientRect().height;
      const chromeHeight = panelHeight - messageHeight - textareaHeight;
      setLimit(composerHeightLimit(panelHeight, chromeHeight, scale));
      handleRef.current?.setAttribute("aria-valuenow", String(Math.round(textareaHeight)));
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
    (value: number | null) => {
      const next = clampComposerHeight(value ?? COMPOSER_AUTO_MAX_HEIGHT, limitRef.current);
      const min = value === null ? minimum : next * scale;
      const node = composerRef.current;
      if (node) {
        footerRef.current?.style.setProperty("--composer-min-height", `${min}px`);
        footerRef.current?.style.setProperty("--composer-max-height", `${next * scale}px`);
        const actual = resizeComposerTextarea(node, min, next * scale);
        handleRef.current?.setAttribute("aria-valuenow", String(Math.round(actual)));
      }
      return next;
    },
    [composerRef, footerRef, minimum, scale],
  );

  useLayoutEffect(() => {
    cancelRef.current?.();
    if (enabled) preview(preferred);
    else {
      footerRef.current?.style.removeProperty("--composer-min-height");
      footerRef.current?.style.removeProperty("--composer-max-height");
    }
    return () => cancelRef.current?.();
  }, [enabled, preferred, limit, preview, footerRef]);

  // The automatic height follows content. Keep the separator's accessible
  // value current without re-rendering the whole composer on every drag frame.
  useLayoutEffect(() => {
    if (enabled && composerRef.current) {
      handleRef.current?.setAttribute(
        "aria-valuenow",
        String(Math.round(composerRef.current.getBoundingClientRect().height)),
      );
    }
  });

  const onPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (!enabled || event.button !== 0 || event.isPrimary === false) return;
      event.preventDefault();
      event.stopPropagation();
      event.currentTarget.focus({ preventScroll: true });
      cancelRef.current?.();
      const pointerId = event.pointerId;
      const startY = event.clientY;
      const startHeight = (composerRef.current?.getBoundingClientRect().height ?? minimum) / scale;
      let pending = startHeight;
      let moved = false;
      const body = document.body;
      const cursor = body.style.cursor;
      const userSelect = body.style.userSelect;
      body.style.cursor = "ns-resize";
      body.style.userSelect = "none";
      setResizing(true);
      const move = (next: globalThis.PointerEvent) => {
        if (next.pointerId !== pointerId) return;
        next.preventDefault();
        if (next.clientY === startY && !moved) return;
        moved = true;
        pending = preview(startHeight + (startY - next.clientY) / scale);
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
        if (commit && moved) save(clampComposerHeight(pending, limitRef.current));
        else preview(preferred);
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
    [enabled, preferred, minimum, composerRef, preview, save, scale],
  );

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!enabled) return;
    if (event.key === "Enter") {
      event.preventDefault();
      cancelRef.current?.();
      save(null);
      return;
    }
    const current = (composerRef.current?.getBoundingClientRect().height ?? minimum) / scale;
    const next =
      event.key === "ArrowUp"
        ? current + 16
        : event.key === "ArrowDown"
          ? current - 16
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
    save(null);
  };
  return {
    handleRef,
    height: height * scale,
    minimum,
    maximum: limit * scale,
    minHeight,
    maxHeight,
    resizing,
    onPointerDown,
    onKeyDown,
    onReset,
  };
}
