import { useLayoutEffect, type RefObject } from "react";

/** Set on the Group's message view; chatMessage.css sizes the list's end room from it. */
export const DOCK_HEIGHT_PROPERTY = "--chat-dock-height";

/** Within this distance of its end, the list counts as showing its newest message. */
const AT_LATEST_PX = 8;

/**
 * On a phone the members float over the end of the message list, which keeps room for them.
 * A line of agent activity above them makes them taller, so the room follows their measured
 * height, and a list showing its newest message moves with it, as one beside the members
 * would. A hidden dock measures nothing and keeps the room it last needed, so hiding it never
 * moves the list.
 */
export function useFloatingDockReserve(
  dock: HTMLElement | null,
  scrollRef: RefObject<HTMLElement | null>,
  enabled: boolean,
) {
  useLayoutEffect(() => {
    if (!enabled || !dock || typeof ResizeObserver === "undefined") return;
    const host = dock.closest<HTMLElement>("[data-group-message-view]");
    if (!host) return;
    const apply = () => {
      const height = Math.ceil(dock.getBoundingClientRect().height);
      if (height <= 0) return;
      const next = `${height}px`;
      if (host.style.getPropertyValue(DOCK_HEIGHT_PROPERTY) === next) return;
      const list = scrollRef.current;
      const atLatest =
        !!list && list.scrollHeight - list.clientHeight - list.scrollTop < AT_LATEST_PX;
      host.style.setProperty(DOCK_HEIGHT_PROPERTY, next);
      // Someone reading earlier messages keeps their place; only the newest stays in view.
      if (list && atLatest) list.scrollTop = list.scrollHeight;
    };
    const observer = new ResizeObserver(apply);
    observer.observe(dock);
    apply();
    return () => {
      observer.disconnect();
      host.style.removeProperty(DOCK_HEIGHT_PROPERTY);
    };
  }, [dock, enabled, scrollRef]);
}
