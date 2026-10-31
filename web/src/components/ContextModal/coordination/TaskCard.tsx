import { memo, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useDraggable } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import { useMinuteClock } from "../../../hooks/useMinuteClock";
import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { formatElapsed, formatFullTime } from "../../../utils/time";
import { useLcdStyle } from "../../../utils/uiStyle";
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
import {
  taskSearchExcerpt,
  taskSearchMatch,
  taskSearchTextMatch,
  type TaskSearchExcerpt,
} from "../taskSearchExcerpt";
import type { ContextModalUi } from "../ui";
import { TaskCardBadges } from "./TaskCardBadges";

interface TaskCardProps {
  task: Task;
  subtasks?: SubtaskProgress;
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  searchQuery?: string;
  selected: boolean;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
  onRestoreTask: (task: Task) => void;
}

interface FoundExcerpt extends Omit<TaskSearchExcerpt, "field"> {
  field: TaskSearchExcerpt["field"] | "assignee" | "priority";
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
  searchQuery = "",
  selected,
  onSelectTask,
  onMoveTaskToStatus,
  onRestoreTask,
}: TaskCardProps) {
  const lcd = useLcdStyle();
  const status = taskStatus(task);
  const title = taskTitle(task);
  const summary = taskDisplaySummary(task);
  // Without a summary, an unfinished task shows the first checklist item still open.
  const openItem =
    !summary && taskIsLive(task)
      ? task.checklist?.find((item) => String(item.text || "").trim() && checklistItemOpen(item))
          ?.text
      : undefined;
  const blocked = taskIsLive(task) && Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
  const query = searchQuery.trim();
  // While searching, quote where the match is when nothing on the card shows
  // it. A card counts only the openings of its clamped title and summary as
  // shown; the LCD row shows its whole title and ID, so it quotes any other
  // field the search matched.
  const found = useMemo((): FoundExcerpt | null => {
    if (!query) return null;
    if (!lcd) {
      return taskSearchExcerpt(task, query, [
        task.id,
        title.slice(0, 30),
        task.assignee,
        task.priority,
        summary.slice(0, 40),
        taskIsLive(task) ? task.handoff_to : "",
      ]);
    }
    const match = taskSearchMatch(task, query);
    return match && match.field !== "title" && match.field !== "id"
      ? { ...match, field: match.field }
      : null;
  }, [task, query, summary, title, lcd]);
  const foundLabel = found
    ? found.field === "handoff_to"
      ? tr("context.handoffTo", "Handoff →")
      : tr("context.searchMatchField", "{{field}}:", {
          field:
            found.field === "title"
              ? tr("context.titleField", "Title")
              : found.field === "notes"
                ? tr("context.notes", "Notes")
                : found.field === "assignee"
                  ? tr("context.assignee", "Assignee")
                  : found.field === "priority"
                    ? tr("context.priority", "Priority")
                    : tr("context.outcome", "Outcome"),
        })
    : null;
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

  const dragHandle = (className?: string) => (
    <button
      ref={setActivatorNodeRef}
      type="button"
      {...attributes}
      {...listeners}
      disabled={syncBusy}
      // The unlayered global button rule overrides Tailwind's touch-none.
      // Keep the browser's pan/back gestures off this drag handle only.
      style={{ touchAction: "none" }}
      className={className}
      onClick={(event) => event.stopPropagation()}
      aria-label={tr("context.dragTaskNamed", "Move {{id}}: {{title}}", { id: task.id, title })}
      title={tr("context.dragTask", "Drag task")}
      data-task-drag={lcd || undefined}
    >
      <span aria-hidden="true">⋮⋮</span>
    </button>
  );
  const runQuickAction = () => {
    if (status === "archived") onRestoreTask(task);
    else onMoveTaskToStatus(task, quickAction.next);
  };

  if (lcd) {
    const titleMatch = query ? taskSearchTextMatch(title, query) : null;
    return (
      <div
        ref={setNodeRef}
        style={{ transform: CSS.Translate.toString(transform) }}
        className={classNames("min-w-0", isDragging && "z-20 opacity-80")}
      >
        <div
          id={`context-task-${task.id}`}
          data-task-id={task.id}
          data-task-row
          data-selected={selected || undefined}
          data-searching={query ? true : undefined}
          onClick={() => onSelectTask(task)}
        >
          {/* The body and the actions are siblings: a touch screen keeps the actions
              in a column beside the text, so they never add a line to the row. */}
          <div data-task-body>
            <button
              type="button"
              title={title}
              onClick={(event) => {
                event.stopPropagation();
                onSelectTask(task);
              }}
              data-task-title
            >
              {/* A span holds the clamp: line clamping is unreliable on a button box. */}
              <span>
                {titleMatch ? (
                  <>
                    {title.slice(0, titleMatch[0])}
                    <mark>{title.slice(titleMatch[0], titleMatch[1])}</mark>
                    {title.slice(titleMatch[1])}
                  </>
                ) : (
                  title
                )}
              </span>
            </button>
            {found ? (
              <p data-task-excerpt>
                <span>{foundLabel}</span> {found.before}
                <mark>{found.match}</mark>
                {found.after}
              </p>
            ) : summary ? (
              <p data-task-summary>{summary}</p>
            ) : openItem ? (
              <p data-task-summary>
                {tr("context.openChecklistItem", "To do: {{item}}", { item: openItem.trim() })}
              </p>
            ) : null}
            <div data-task-meta>
              <span data-task-meta-id>
                {task.id}
                <TaskUpdatedAge updatedAt={task.updated_at} tr={tr} />
              </span>
              <TaskCardBadges task={task} tr={tr} subtasks={subtasks} lcd />
            </div>
          </div>
          <span data-task-actions onClick={(event) => event.stopPropagation()}>
            {dragHandle()}
            <button type="button" onClick={runQuickAction} disabled={syncBusy} data-task-quick>
              {quickAction.label}
            </button>
          </span>
        </div>
      </div>
    );
  }

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
            title={title}
            onClick={(event) => {
              event.stopPropagation();
              onSelectTask(task);
            }}
            className="min-w-0 flex-1 rounded-sm text-left focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
          >
            <span className="line-clamp-2 text-sm font-semibold text-[var(--color-text-primary)] [overflow-wrap:anywhere]">
              {title}
            </span>
            <span className={classNames("mt-1 block text-xs", ui.mutedTextClass)}>
              {task.id}
              <TaskUpdatedAge updatedAt={task.updated_at} tr={tr} />
            </span>
          </button>
          {dragHandle(
            "touch-none rounded-lg px-2 py-1 text-[11px] glass-btn text-[var(--color-text-secondary)] focus:opacity-100 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] md:opacity-0 md:group-hover/task:opacity-100 md:[@media(any-pointer:coarse)]:opacity-100",
          )}
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

        {found ? (
          <p
            className={classNames(
              "mt-1.5 line-clamp-2 text-xs [overflow-wrap:anywhere]",
              ui.subtleTextClass,
            )}
          >
            <span className="font-medium">{foundLabel}</span> {found.before}
            <mark className="rounded bg-amber-200 px-0.5 text-amber-900 dark:bg-amber-500/20 dark:text-amber-200">
              {found.match}
            </mark>
            {found.after}
          </p>
        ) : null}

        <TaskCardBadges task={task} tr={tr} subtasks={subtasks} />

        <div className="mt-2 flex items-center" onClick={(event) => event.stopPropagation()}>
          <button
            type="button"
            onClick={runQuickAction}
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
    previous.searchQuery === next.searchQuery &&
    previous.selected === next.selected &&
    previous.onSelectTask === next.onSelectTask &&
    previous.onMoveTaskToStatus === next.onMoveTaskToStatus &&
    previous.onRestoreTask === next.onRestoreTask
  );
});
