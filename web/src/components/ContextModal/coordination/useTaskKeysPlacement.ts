import { useEffect, type RefObject } from "react";

/**
 * A row's hover keys straddle its top rule. When that rule meets the pinned
 * toolbar's bottom edge, the keys' upper half would sit under the toolbar, so the
 * row raises them over it (`data-task-keys-raised`). That holds while the row's
 * text is in view, its rule at most its top padding under the toolbar, so the keys
 * reach no further into the toolbar than the padding below its controls' labels.
 * Once the text scrolls under, the keys stay under with it. The keys never leave
 * their own rule, so two rows' keys never meet. Only the rows whose keys can show
 * are placed, the one under the pointer and the one holding focus, and again
 * whenever the board scrolls or its layout moves them.
 */
export function useTaskKeysPlacement(boardRef: RefObject<HTMLElement | null>, enabled: boolean) {
  useEffect(() => {
    const board = boardRef.current;
    const scroll = board?.closest<HTMLElement>("[data-context-scroll]");
    if (!enabled || !board || !scroll) return;
    const section = board.closest<HTMLElement>("[data-taskboard]") ?? board;
    let hovered: HTMLElement | null = null;
    let focused: HTMLElement | null = null;

    const place = (row: HTMLElement | null) => {
      const keys = row?.querySelector<HTMLElement>("[data-task-actions]");
      if (!row || !keys) return;
      // The toolbar's edge as laid out now, read rather than taken from its
      // measured height, which updates after a scroll that its growth causes.
      // Below 768px the outer layer has no box and only the jump layer pins.
      let toolbarBottom = scroll.getBoundingClientRect().top + scroll.clientTop;
      for (const layer of section.querySelectorAll<HTMLElement>(
        "[data-taskboard-sticky], [data-taskboard-jump-layer]",
      )) {
        const rect = layer.getBoundingClientRect();
        if (rect.height > 0) toolbarBottom = Math.max(toolbarBottom, rect.bottom);
      }
      const top = row.getBoundingClientRect().top;
      const textTop = top + parseFloat(getComputedStyle(row).paddingTop);
      row.toggleAttribute(
        "data-task-keys-raised",
        textTop >= toolbarBottom && top - keys.offsetHeight / 2 < toolbarBottom,
      );
    };
    const onPointerOver = (event: Event) => {
      hovered = (event.target as Element).closest<HTMLElement>("[data-task-row]");
      place(hovered);
    };
    const onFocusIn = (event: Event) => {
      focused = (event.target as Element).closest<HTMLElement>("[data-task-row]");
      place(focused);
    };
    const placeShown = () => {
      place(hovered);
      place(focused);
    };
    // A row can also reach the toolbar without a scroll. Content changes (the
    // minute clock rewrapping a row above, rows arriving or re-sorting, a new
    // objective) can leave every box the same size, in a stretched grid column or
    // when one row shrinks as another grows, so they are watched as changes. A
    // wider window or text size rewraps without one; that resizes the section,
    // and scroll anchoring stands aside on a resize.
    const changes = new MutationObserver(placeShown);
    const resizes = new ResizeObserver(placeShown);

    board.addEventListener("pointerover", onPointerOver);
    board.addEventListener("focusin", onFocusIn);
    scroll.addEventListener("scroll", placeShown, { passive: true });
    changes.observe(scroll, { childList: true, characterData: true, subtree: true });
    resizes.observe(section);
    return () => {
      board.removeEventListener("pointerover", onPointerOver);
      board.removeEventListener("focusin", onFocusIn);
      scroll.removeEventListener("scroll", placeShown);
      changes.disconnect();
      resizes.disconnect();
    };
  }, [boardRef, enabled]);
}
