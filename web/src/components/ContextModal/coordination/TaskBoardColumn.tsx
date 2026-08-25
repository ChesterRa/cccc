import { useDroppable } from "@dnd-kit/core";
import type { Task } from "../../../types";
import { TASK_PAGE_SIZE } from "../../../features/contextModal/taskBoardData";
import { classNames } from "../../../utils/classNames";
import type { BoardStatus, ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";
import { TaskCard } from "./TaskCard";

export const TASK_COLUMN_PAGE_SIZE = TASK_PAGE_SIZE;

export function TaskBoardColumn({
  columnKey,
  label,
  items,
  totalCount,
  hasMore,
  loading,
  tr,
  ui,
  syncBusy,
  selectedTaskId,
  onSelectTask,
  onMoveTaskToStatus,
  onLoadMore,
}: {
  columnKey: BoardStatus;
  label: string;
  items: Task[];
  totalCount: number;
  hasMore: boolean;
  loading: boolean;
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  selectedTaskId: string;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
  onLoadMore: (status: BoardStatus) => void;
}) {
  const { setNodeRef, isOver } = useDroppable({
    id: `column:${columnKey}`,
    data: { type: "column", status: columnKey },
  });
  const remainingCount = Math.max(0, totalCount - items.length);
  const nextCount = Math.min(TASK_COLUMN_PAGE_SIZE, remainingCount);

  return (
    <section
      ref={setNodeRef}
      data-task-column={columnKey}
      data-rendered-task-count={items.length}
      data-total-task-count={totalCount}
      className={classNames(
        "min-w-0 rounded-2xl border p-3 transition-all",
        isOver
          ? "border-black/10 bg-[rgb(245,245,245)] shadow-[0_0_0_1px_rgba(17,24,39,0.08)] dark:border-white/12 dark:bg-white/[0.08] dark:shadow-[0_0_0_1px_rgba(255,255,255,0.06)]"
          : "glass-panel",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-[var(--color-text-primary)]">{label}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>
            {totalCount} {tr("context.items", "items")}
          </div>
        </div>
        <span className="rounded-full px-2 py-0.5 text-[11px] glass-panel text-[var(--color-text-tertiary)]">
          {totalCount}
        </span>
      </div>
      <div className="mt-3 space-y-2">
        {items.length > 0 ? (
          items.map((task) => (
            <TaskCard
              key={task.id}
              task={task}
              tr={tr}
              ui={ui}
              syncBusy={syncBusy}
              selected={selectedTaskId === task.id}
              onSelectTask={onSelectTask}
              onMoveTaskToStatus={onMoveTaskToStatus}
            />
          ))
        ) : (
          <div className="rounded-lg border border-dashed border-[var(--glass-border-subtle)] px-3 py-5 text-xs text-[var(--color-text-muted)]">
            {tr(`context.empty.${columnKey}`, "No tasks here")}
          </div>
        )}
      </div>
      {hasMore ? (
        <button
          type="button"
          onClick={() => onLoadMore(columnKey)}
          disabled={loading}
          className={classNames(ui.buttonSecondaryClass, "mt-3 w-full justify-center")}
        >
          {loading
            ? tr("context.loadingTasks", "Loading tasks…")
            : tr("context.showMoreTasks", "Show {{count}} more", { count: nextCount })}
        </button>
      ) : null}
    </section>
  );
}
