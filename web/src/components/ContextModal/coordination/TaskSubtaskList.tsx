import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { statusTone, taskStatus, taskTitle, type ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";

export function TaskSubtaskList({
  tr,
  ui,
  subtasks,
  onOpenTask,
}: {
  tr: ContextTranslator;
  ui: ContextModalUi;
  subtasks: readonly Task[];
  onOpenTask: (taskId: string) => void;
}) {
  if (subtasks.length === 0) return null;
  const done = subtasks.filter((task) => taskStatus(task) === "done").length;

  return (
    <div className={classNames("rounded-xl border px-3 py-3", "glass-panel")}>
      <div className="flex items-center justify-between gap-2">
        <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>
          {tr("context.subtasks", "Subtasks")}
        </div>
        <span
          className={classNames(
            "rounded-full px-2 py-0.5 text-[11px] font-medium",
            "glass-panel text-[var(--color-text-secondary)]",
          )}
        >
          {tr("context.subtasksDone", "{{done}} of {{total}} done", {
            done,
            total: subtasks.length,
          })}
        </span>
      </div>
      <ul className="mt-2 space-y-1">
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
                  {status}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className={classNames("mt-2 text-xs", ui.mutedTextClass)}>
        {tr(
          "context.subtasksHint",
          "Counts direct subtasks; archived ones are excluded. Subtask status does not complete this task.",
        )}
      </div>
    </div>
  );
}
