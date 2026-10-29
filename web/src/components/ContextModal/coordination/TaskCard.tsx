import { memo } from "react";
import { useTranslation } from "react-i18next";
import { useDraggable } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import { useMinuteClock } from "../../../hooks/useMinuteClock";
import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { formatElapsed, formatFullTime } from "../../../utils/time";
import {
  checklistItemOpen,
  taskDisplaySummary,
  taskIsLive,
  taskStatus,
  taskTitle,
  type BoardStatus,
  type ContextTranslator,
  type SubtaskProgress,
} from "../model";
import type { ContextModalUi } from "../ui";
import { TaskCardBadges } from "./TaskCardBadges";

interface TaskCardProps {
  task: Task;
  subtasks?: SubtaskProgress;
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  selected: boolean;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
  onRestoreTask: (task: Task) => void;
}

function sameTaskRevision(left: Task, right: Task): boolean {
  return (
    left === right ||
    (left.id === right.id &&
      left.rev === right.rev &&
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

/** By this device's clock, so a time it cannot place, or one ahead of it, shows nothing. */
function TaskUpdatedAge({ updatedAt, tr }: { updatedAt?: string | null; tr: ContextTranslator }) {
  const { i18n } = useTranslation();
  useMinuteClock();
  const seconds = Math.floor((Date.now() - Date.parse(updatedAt || "")) / 1000);
  if (!Number.isSafeInteger(seconds) || seconds < 0) return null;
  return (
    <>
      {" · "}
      <time dateTime={updatedAt ?? undefined} title={formatFullTime(updatedAt ?? undefined)}>
        {seconds < 60
          ? tr("context.updatedJustNow", "Updated just now")
          : tr("context.updatedAgo", "Updated {{elapsed}}", {
              elapsed: formatElapsed(seconds, i18n?.language),
            })}
      </time>
    </>
  );
}

function TaskCardComponent({
  task,
  subtasks,
  tr,
  ui,
  syncBusy,
  selected,
  onSelectTask,
  onMoveTaskToStatus,
  onRestoreTask,
}: TaskCardProps) {
  const status = taskStatus(task);
  const summary = taskDisplaySummary(task);
  // Without a summary, an unfinished task shows the first checklist item still open.
  const openItem =
    !summary && taskIsLive(task)
      ? task.checklist?.find((item) => String(item.text || "").trim() && checklistItemOpen(item))
          ?.text
      : undefined;
  const blocked = taskIsLive(task) && Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, isDragging } =
    useDraggable({
      id: `task:${task.id}`,
      disabled: syncBusy,
      data: { type: "task", taskId: task.id, status },
      attributes: { roleDescription: tr("context.dragRoleDescription", "movable task") },
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
          <button
            type="button"
            title={taskTitle(task)}
            onClick={(event) => {
              event.stopPropagation();
              onSelectTask(task);
            }}
            className="min-w-0 flex-1 rounded-sm text-left focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
          >
            <span className="line-clamp-2 text-sm font-semibold text-[var(--color-text-primary)] [overflow-wrap:anywhere]">
              {taskTitle(task)}
            </span>
            <span className={classNames("mt-1 block text-xs", ui.mutedTextClass)}>
              {task.id}
              <TaskUpdatedAge updatedAt={task.updated_at} tr={tr} />
            </span>
          </button>
          <button
            ref={setActivatorNodeRef}
            type="button"
            {...attributes}
            {...listeners}
            disabled={syncBusy}
            className="touch-none rounded-lg px-2 py-1 text-[11px] glass-btn text-[var(--color-text-secondary)] focus:opacity-100 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] md:opacity-0 md:group-hover/task:opacity-100"
            onClick={(event) => event.stopPropagation()}
            aria-label={tr("context.dragTaskNamed", "Move {{id}}: {{title}}", {
              id: task.id,
              title: taskTitle(task),
            })}
            title={tr("context.dragTask", "Drag task")}
          >
            <span aria-hidden="true">⋮⋮</span>
          </button>
        </div>

        {summary ? (
          <div className={classNames("mt-2 line-clamp-3 text-xs", ui.subtleTextClass)}>
            {summary}
          </div>
        ) : openItem ? (
          <div className={classNames("mt-2 line-clamp-2 text-xs", ui.subtleTextClass)}>
            {tr("context.openChecklistItem", "To do: {{item}}", { item: openItem.trim() })}
          </div>
        ) : null}

        <TaskCardBadges task={task} tr={tr} subtasks={subtasks} />

        <div className="mt-2 flex items-center" onClick={(event) => event.stopPropagation()}>
          <button
            type="button"
            onClick={() => {
              if (status === "archived") onRestoreTask(task);
              else onMoveTaskToStatus(task, quickAction.next);
            }}
            disabled={syncBusy}
            className="rounded-md border border-[var(--glass-border-subtle)] px-2.5 py-1.5 text-xs font-medium text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] disabled:opacity-50"
          >
            {quickAction.label}
          </button>
        </div>
      </div>
    </div>
  );
}

export const TaskCard = memo(TaskCardComponent, (previous, next) => {
  return (
    sameTaskRevision(previous.task, next.task) &&
    previous.subtasks?.done === next.subtasks?.done &&
    previous.subtasks?.total === next.subtasks?.total &&
    previous.tr === next.tr &&
    previous.ui === next.ui &&
    previous.syncBusy === next.syncBusy &&
    previous.selected === next.selected &&
    previous.onSelectTask === next.onSelectTask &&
    previous.onMoveTaskToStatus === next.onMoveTaskToStatus &&
    previous.onRestoreTask === next.onRestoreTask
  );
});
