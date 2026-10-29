import { Plus } from "lucide-react";
import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import {
  statusTone,
  taskStatus,
  taskStatusLabel,
  taskTitle,
  type ContextTranslator,
} from "../model";
import type { ContextModalUi } from "../ui";

export function TaskSubtaskList({
  tr,
  ui,
  subtasks,
  onOpenTask,
  onAddSubtask,
}: {
  tr: ContextTranslator;
  ui: ContextModalUi;
  subtasks: readonly Task[];
  onOpenTask: (taskId: string) => void;
  onAddSubtask?: () => void;
}) {
  if (subtasks.length === 0 && !onAddSubtask) return null;
  const done = subtasks.filter((task) => taskStatus(task) === "done").length;

  return (
    <div className="space-y-2 rounded-xl border border-[var(--glass-border-subtle)] px-3 py-3">
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <div className="text-sm font-semibold text-[var(--color-text-primary)]">
            {tr("context.subtasks", "Subtasks")}
          </div>
          {subtasks.length > 0 ? (
            <span className={classNames("text-xs", ui.mutedTextClass)}>
              {tr("context.subtasksDone", "{{done}} of {{total}} done", {
                done,
                total: subtasks.length,
              })}
            </span>
          ) : null}
        </div>
        {onAddSubtask ? (
          <button
            type="button"
            onClick={onAddSubtask}
            className={classNames(ui.buttonSecondaryClass, "gap-1.5")}
          >
            <Plus className="size-4" aria-hidden="true" />
            {tr("context.addSubtask", "Add subtask")}
          </button>
        ) : null}
      </div>
      {subtasks.length > 0 ? (
        <>
          <ul className="space-y-1">
            {subtasks.map((subtask) => {
              const status = taskStatus(subtask);
              return (
                <li key={subtask.id}>
                  <button
                    type="button"
                    onClick={() => onOpenTask(subtask.id)}
                    className={classNames(
                      "flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs transition-colors",
                      "hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
                    )}
                  >
                    <span className={classNames("shrink-0", ui.mutedTextClass)}>{subtask.id}</span>
                    <span className="min-w-0 flex-1 truncate text-[var(--color-text-primary)]">
                      {taskTitle(subtask)}
                    </span>
                    <span
                      className={classNames(
                        "shrink-0 rounded-full border px-2 py-0.5 text-[11px] font-medium",
                        statusTone(status),
                      )}
                    >
                      {taskStatusLabel(status, tr)}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
          <div className={classNames("text-xs", ui.mutedTextClass)}>
            {tr(
              "context.subtasksHint",
              "Counts direct subtasks; archived ones are excluded. Subtask status does not complete this task.",
            )}
          </div>
        </>
      ) : null}
    </div>
  );
}
