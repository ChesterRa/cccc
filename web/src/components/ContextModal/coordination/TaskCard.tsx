import { memo } from "react";
import { useDraggable } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import {
  statusTone,
  taskDisplaySummary,
  taskStatus,
  taskTitle,
  type BoardStatus,
  type ContextTranslator,
} from "../model";
import type { ContextModalUi } from "../ui";
import { TaskCardBadges } from "./TaskCardBadges";

interface TaskCardProps {
  task: Task;
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  selected: boolean;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
}

function sameTaskRevision(left: Task, right: Task): boolean {
  return (
    left === right ||
    (left.id === right.id &&
      left.title === right.title &&
      left.outcome === right.outcome &&
      left.parent_id === right.parent_id &&
      left.status === right.status &&
      left.assignee === right.assignee &&
      left.priority === right.priority &&
      left.waiting_on === right.waiting_on &&
      left.handoff_to === right.handoff_to &&
      left.task_type === right.task_type &&
      left.notes === right.notes &&
      left.updated_at === right.updated_at &&
      JSON.stringify(left.blocked_by) === JSON.stringify(right.blocked_by) &&
      JSON.stringify(left.checklist) === JSON.stringify(right.checklist))
  );
}

function TaskCardComponent({
  task,
  tr,
  ui,
  syncBusy,
  selected,
  onSelectTask,
  onMoveTaskToStatus,
}: TaskCardProps) {
  const status = taskStatus(task);
  const blocked = Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: `task:${task.id}`,
    disabled: syncBusy,
    data: { type: "task", taskId: task.id, status },
  });
  const quickAction =
    status === "planned"
      ? { label: tr("context.start", "Start"), next: "active" as BoardStatus }
      : status === "active"
        ? { label: tr("context.done", "Done"), next: "done" as BoardStatus }
        : status === "done"
          ? { label: tr("context.reopen", "Reopen"), next: "active" as BoardStatus }
          : { label: tr("context.restore", "Restore"), next: "planned" as BoardStatus };

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform) }}
      className={classNames("group/task min-w-0", isDragging && "z-20 opacity-80")}
    >
      <div
        id={`context-task-${task.id}`}
        data-task-id={task.id}
        {...attributes}
        onClick={() => onSelectTask(task)}
        className={classNames(
          "min-w-0 w-full cursor-pointer overflow-hidden rounded-2xl border p-3 text-left transition-all",
          blocked
            ? "border-rose-500/30 bg-rose-500/5"
            : selected
              ? "border-black/10 bg-[rgb(245,245,245)] shadow-[0_0_0_1px_rgba(17,24,39,0.08)] dark:border-white/12 dark:bg-white/[0.08] dark:shadow-[0_0_0_1px_rgba(255,255,255,0.06)]"
              : "glass-panel hover:border-[var(--glass-border-subtle)]",
        )}
      >
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1 text-left">
            <div className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
              {taskTitle(task)}
            </div>
            <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{task.id}</div>
          </div>
          <div className="flex items-center gap-2">
            <span
              className={classNames(
                "rounded-full border px-2 py-0.5 text-[11px] font-medium",
                statusTone(status),
              )}
            >
              {status}
            </span>
            <button
              type="button"
              {...listeners}
              className="rounded-lg px-2 py-1 text-[11px] glass-btn text-[var(--color-text-secondary)] md:opacity-0 md:group-hover/task:opacity-100"
              onClick={(event) => event.stopPropagation()}
              aria-label={tr("context.dragTask", "Drag task")}
              title={tr("context.dragTask", "Drag task")}
            >
              ⋮⋮
            </button>
          </div>
        </div>

        {taskDisplaySummary(task) ? (
          <div className={classNames("mt-2 line-clamp-3 text-xs", ui.subtleTextClass)}>
            {taskDisplaySummary(task)}
          </div>
        ) : null}

        <TaskCardBadges task={task} tr={tr} />

        <div
          className="mt-3 flex items-center gap-2 border-t pt-3"
          onClick={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            onClick={() => onMoveTaskToStatus(task, quickAction.next)}
            disabled={syncBusy}
            className={ui.buttonSecondaryClass}
          >
            {quickAction.label}
          </button>
          <button
            type="button"
            onClick={() => onSelectTask(task)}
            className={ui.buttonSecondaryClass}
          >
            {tr("context.edit", "Edit")}
          </button>
        </div>
      </div>
    </div>
  );
}

export const TaskCard = memo(TaskCardComponent, (previous, next) => {
  return (
    sameTaskRevision(previous.task, next.task) &&
    previous.tr === next.tr &&
    previous.ui === next.ui &&
    previous.syncBusy === next.syncBusy &&
    previous.selected === next.selected &&
    previous.onSelectTask === next.onSelectTask &&
    previous.onMoveTaskToStatus === next.onMoveTaskToStatus
  );
});
