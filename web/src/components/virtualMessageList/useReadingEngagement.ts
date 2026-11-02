import { useEffect, useRef, type RefObject } from "react";
import type { ChatVisibleMessage } from "../../stores/useUIStore";

// Focus that follows a Tab press this soon is keyboard navigation, not a programmatic focus.
const TAB_FOCUS_WINDOW_MS = 500;
// A scroll this soon after a wheel, touch drag, scroll key or press on the log itself is the
// reader's own, not a restore or auto-follow.
const SCROLL_INTENT_WINDOW_MS = 300;
// Keys that scroll the log unless an editable field takes them.
const SCROLL_KEYS = new Set(["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown", " "]);

function isEditable(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))
  );
}

function messageRowId(target: EventTarget | null): string {
  const row =
    target instanceof Element ? target.closest<HTMLElement>('[data-message-row="true"]') : null;
  return String(row?.dataset.messageId || "").trim();
}

/**
 * Whether the reader has acted in this view (scrolled, or touched, typed in or tabbed to a
 * message) and which message they touched last, until they scroll the list themselves.
 * Restoration, auto-follow and programmatic focus never count, so a view that is only looked
 * at leaves the saved place alone.
 */
export function useReadingEngagement(parentRef: RefObject<HTMLElement | null>) {
  const engagedRef = useRef(false);
  const touchedIdRef = useRef("");
  const scrollIntentAtRef = useRef(0);
  const scrollIntentStartTopRef = useRef(0);
  const scrollIntentIdRef = useRef(0);
  useEffect(() => {
    const el = parentRef.current;
    if (!el) return undefined;
    let lastTabAt = 0;
    const engage = () => {
      engagedRef.current = true;
    };
    const intendScroll = () => {
      engage();
      scrollIntentAtRef.current = Date.now();
      scrollIntentStartTopRef.current = el.scrollTop;
      scrollIntentIdRef.current += 1;
    };
    const touch = (event: Event) => {
      engage();
      const id = messageRowId(event.target);
      if (id) touchedIdRef.current = id;
    };
    // A press on the log itself, outside every message, is its scrollbar or blank space.
    const onPointerDown = (event: Event) => (event.target === el ? intendScroll() : touch(event));
    // A key on a message touches it; a scroll key that the browser hands on to the log, as
    // from a focused button, then scrolls it and clears that touch like any other scroll.
    const onKeyDown = (event: KeyboardEvent) => {
      touch(event);
      if (SCROLL_KEYS.has(event.key) && !isEditable(event.target)) intendScroll();
    };
    // Once the reader scrolls the list, the message they touched gives way to the middle one.
    // A wheel inside a message's own scroller moves nothing here and keeps it.
    const onScroll = () => {
      if (Date.now() - scrollIntentAtRef.current <= SCROLL_INTENT_WINDOW_MS)
        touchedIdRef.current = "";
    };
    const noteTab = (event: KeyboardEvent) => {
      if (event.key === "Tab") lastTabAt = Date.now();
    };
    const onFocusIn = (event: FocusEvent) => {
      if (Date.now() - lastTabAt <= TAB_FOCUS_WINDOW_MS) touch(event);
    };
    el.addEventListener("wheel", intendScroll, { passive: true });
    el.addEventListener("touchmove", intendScroll, { passive: true });
    el.addEventListener("pointerdown", onPointerDown);
    el.addEventListener("keydown", onKeyDown);
    el.addEventListener("focusin", onFocusIn);
    el.addEventListener("scroll", onScroll, { passive: true });
    document.addEventListener("keydown", noteTab, true);
    return () => {
      el.removeEventListener("wheel", intendScroll);
      el.removeEventListener("touchmove", intendScroll);
      el.removeEventListener("pointerdown", onPointerDown);
      el.removeEventListener("keydown", onKeyDown);
      el.removeEventListener("focusin", onFocusIn);
      el.removeEventListener("scroll", onScroll);
      document.removeEventListener("keydown", noteTab, true);
    };
  }, [parentRef]);
  return {
    engagedRef,
    touchedIdRef,
    scrollIntentAtRef,
    scrollIntentStartTopRef,
    scrollIntentIdRef,
  };
}

export type ReadingFocus = {
  focusId: string;
  focusOffsetPx: number;
  /** Every message on screen, most likely read first: the touched one, then by nearness to the
   * middle. Another view keeps the first of these it shows, at the same height. */
  visible: ChatVisibleMessage[];
  /** Whether the first visible message is the one the reader touched. */
  touched: boolean;
};

/**
 * The message being read: the one last touched while it is still on screen, even in part
 * (scrolling the list clears it), else the one nearest the middle of the reading area, measured
 * from the part of it on screen so a long message across the middle counts as at it. Rows
 * `canFocus` rejects (optimistic ones) never count. Offsets follow the anchor convention:
 * container top minus row top.
 */
export function getReadingFocus(
  container: HTMLElement,
  topInset: number,
  touchedId: string,
  canFocus: (id: string) => boolean,
): ReadingFocus | null {
  const box = container.getBoundingClientRect();
  const top = box.top + topInset;
  const middle = (top + box.bottom) / 2;
  const rows = Array.from(container.querySelectorAll<HTMLElement>('[data-message-row="true"]'))
    .map((row) => {
      const rect = row.getBoundingClientRect();
      const shownTop = Math.max(rect.top, top);
      const shownBottom = Math.min(rect.bottom, box.bottom);
      return {
        id: String(row.dataset.messageId || "").trim(),
        offsetPx: box.top - rect.top,
        distance:
          middle < shownTop ? shownTop - middle : middle > shownBottom ? middle - shownBottom : 0,
        visible: rect.bottom > top + 1 && rect.top < box.bottom - 1,
      };
    })
    .filter((row) => row.id && row.visible && canFocus(row.id));
  if (!rows.length) return null;
  const touched = rows.find((row) => touchedId && row.id === touchedId);
  const ordered = [...rows].sort((a, b) => a.distance - b.distance);
  const visible = (touched ? [touched, ...ordered.filter((row) => row !== touched)] : ordered).map(
    ({ id, offsetPx }) => ({ id, offsetPx }),
  );
  return {
    focusId: visible[0].id,
    focusOffsetPx: visible[0].offsetPx,
    visible,
    touched: !!touched,
  };
}
