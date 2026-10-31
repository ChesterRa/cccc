import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import type { TaskColumnPages } from "../../../features/contextModal/taskBoardData";
import { taskStatusLabel, type BoardStatus, type ContextTranslator } from "../model";

const PRIMARY_COLUMNS: BoardStatus[] = ["active", "planned", "done"];

export function TaskColumnJump({
  boardRef,
  columnPages,
  archivedExpanded,
  tr,
}: {
  boardRef: RefObject<HTMLElement | null>;
  columnPages: TaskColumnPages;
  archivedExpanded: boolean;
  tr: ContextTranslator;
}) {
  const columns = archivedExpanded ? [...PRIMARY_COLUMNS, "archived" as const] : PRIMARY_COLUMNS;
  const navRef = useRef<HTMLElement>(null);
  const [moreRight, setMoreRight] = useState(false);

  function updateOverflow() {
    const nav = navRef.current;
    setMoreRight(Boolean(nav && nav.scrollWidth - nav.scrollLeft - nav.clientWidth > 1));
  }

  useLayoutEffect(() => {
    const nav = navRef.current;
    if (!nav) return;
    updateOverflow();
    const observer = new ResizeObserver(updateOverflow);
    observer.observe(nav);
    for (const child of nav.children) observer.observe(child);
    return () => observer.disconnect();
  }, [archivedExpanded, columnPages, tr]);

  function jumpTo(column: BoardStatus) {
    const board = boardRef.current;
    const scroll = board?.closest<HTMLElement>("[data-context-scroll]");
    const target = board?.querySelector<HTMLElement>(`[data-task-column="${column}"]`);
    const sticky = navRef.current?.closest<HTMLElement>("[data-taskboard-jump-layer]");
    if (!board || !scroll || !target || !sticky) return;

    const stickyHeight = sticky.getBoundingClientRect().height;
    const viewportTop = scroll.getBoundingClientRect().top + scroll.clientTop;
    const targetTop = target.getBoundingClientRect().top;
    scroll.scrollTo({
      top: Math.max(0, scroll.scrollTop + targetTop - viewportTop - stickyHeight - 8),
      behavior: "auto",
    });
    // Without this, pressing Tab after a jump returns to the toolbar and pulls
    // the viewport back up, instead of continuing into the target column.
    target.focus({ preventScroll: true });
  }

  return (
    <div className="relative min-w-0 md:hidden">
      <nav
        ref={navRef}
        aria-label={tr("context.tasks", "Tasks")}
        data-taskboard-jump=""
        onScroll={updateOverflow}
        className="scrollbar-subtle flex min-w-0 overflow-x-auto"
      >
        {columns.map((column) => (
          <button
            type="button"
            key={column}
            data-taskboard-jump-column={column}
            onClick={() => jumpTo(column)}
            className="shrink-0 whitespace-nowrap px-2 py-1 text-xs"
          >
            {taskStatusLabel(column, tr)}{" "}
            {columnPages[column].loading ? "…" : columnPages[column].totalCount}
          </button>
        ))}
      </nav>
      {moreRight ? (
        <span
          aria-hidden="true"
          data-taskboard-jump-more=""
          className="pointer-events-none absolute inset-y-0 right-0 flex items-center bg-[var(--lcd-bg)] px-0.5"
        >
          ›
        </span>
      ) : null}
    </div>
  );
}
