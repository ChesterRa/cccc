import { useLayoutEffect, useMemo, useState, type RefObject } from "react";
import type { BoardStatus } from "../model";

// TaskBoardGrid gets a second column at this board width (`@min-[540px]/task-board`).
const TWO_COLUMN_BOARD_WIDTH = 540;

/**
 * The board's column order, shared by the grid and the column jump keys. A single-column board
 * puts Active first; two or more columns read Planned, Active, Done. The order follows the
 * board's own width, the same signal that sets the column count.
 */
export function useBoardColumnOrder(
  board: RefObject<HTMLElement | null>,
  shown: boolean,
  archivedExpanded: boolean,
): BoardStatus[] {
  const [singleColumn, setSingleColumn] = useState(true);
  useLayoutEffect(() => {
    const element = board.current;
    if (!shown || !element) return;
    // Measured before the first paint, so a wide board never shows the single-column order.
    setSingleColumn(element.clientWidth < TWO_COLUMN_BOARD_WIDTH);
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setSingleColumn(entry.contentRect.width < TWO_COLUMN_BOARD_WIDTH);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [board, shown]);
  return useMemo(() => {
    const order: BoardStatus[] = singleColumn
      ? ["active", "planned", "done"]
      : ["planned", "active", "done"];
    return archivedExpanded ? [...order, "archived"] : order;
  }, [singleColumn, archivedExpanded]);
}
