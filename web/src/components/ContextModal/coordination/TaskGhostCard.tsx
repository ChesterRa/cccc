import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import {
  taskDisplaySummary,
  taskStatus,
  taskStatusLabel,
  taskTitle,
  type ContextTranslator,
} from "../model";
import { TaskCardBadges } from "./TaskCardBadges";

export function TaskGhostCard({
  task,
  tr,
  mutedTextClass,
  subtleTextClass,
}: {
  task: Task;
  tr: ContextTranslator;
  mutedTextClass: string;
  subtleTextClass: string;
}) {
  const status = taskStatus(task);

  return (
    <div
      data-task-ghost
      className={classNames(
        "w-[320px] max-w-[calc(100vw-2rem)] rounded-2xl border p-3 shadow-2xl",
        "glass-panel",
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="line-clamp-2 text-sm font-semibold text-[var(--color-text-primary)] [overflow-wrap:anywhere]">
            {taskTitle(task)}
          </div>
          <div className={classNames("mt-1 text-xs", mutedTextClass)}>{task.id}</div>
        </div>
        <span data-task-ghost-status className="border px-2 py-0.5 text-[11px] font-medium">
          {taskStatusLabel(status, tr)}
        </span>
      </div>
      {taskDisplaySummary(task) ? (
        <div className={classNames("mt-2 line-clamp-3 text-xs", subtleTextClass)}>
          {taskDisplaySummary(task)}
        </div>
      ) : null}
      <div data-task-meta className="mt-3">
        <TaskCardBadges task={task} tr={tr} compact />
      </div>
    </div>
  );
}
