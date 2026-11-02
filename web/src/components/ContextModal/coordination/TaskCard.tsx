import { memo, useMemo } from "react";
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
import { taskSearchMatch, taskSearchTextMatch } from "../taskSearchExcerpt";
import { TaskCardBadges } from "./TaskCardBadges";

interface TaskCardProps {
  task: Task;
  subtasks?: SubtaskProgress;
  tr: ContextTranslator;
  syncBusy: boolean;
  searchQuery?: string;
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
  syncBusy,
  searchQuery = "",
  selected,
  onSelectTask,
  onMoveTaskToStatus,
  onRestoreTask,
}: TaskCardProps) {
  const status = taskStatus(task);
  const title = taskTitle(task);
  const summary = taskDisplaySummary(task);
  // Without a summary, an unfinished task shows the first checklist item still open.
  const openItem =
    !summary && taskIsLive(task)
      ? task.checklist?.find((item) => String(item.text || "").trim() && checklistItemOpen(item))
          ?.text
      : undefined;
  const query = searchQuery.trim();
  // The row shows its title and ID; quote a match from any other field.
  const found = useMemo(() => {
    if (!query) return null;
    const match = taskSearchMatch(task, query);
    return match && match.field !== "title" && match.field !== "id" ? match : null;
  }, [task, query]);
  const foundLabel = found
    ? found.field === "handoff_to"
      ? tr("context.handoffTo", "Handoff →")
      : tr("context.searchMatchField", "{{field}}:", {
          field:
            found.field === "notes"
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

  const dragHandle = () => (
    <button
      ref={setActivatorNodeRef}
      type="button"
      {...attributes}
      {...listeners}
      disabled={syncBusy}
      // The unlayered global button rule overrides Tailwind's touch-none.
      // Keep the browser's pan/back gestures off this drag handle only.
      style={{ touchAction: "none" }}
      onClick={(event) => event.stopPropagation()}
      aria-label={tr("context.dragTaskNamed", "Move {{id}}: {{title}}", { id: task.id, title })}
      title={tr("context.dragTask", "Drag task")}
      data-task-drag
    >
      <span aria-hidden="true">⋮⋮</span>
    </button>
  );
  const runQuickAction = () => {
    if (status === "archived") onRestoreTask(task);
    else onMoveTaskToStatus(task, quickAction.next);
  };

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
            <TaskCardBadges task={task} tr={tr} subtasks={subtasks} />
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

export const TaskCard = memo(TaskCardComponent, (previous, next) => {
  return (
    sameTaskRevision(previous.task, next.task) &&
    previous.subtasks?.done === next.subtasks?.done &&
    previous.subtasks?.total === next.subtasks?.total &&
    previous.tr === next.tr &&
    previous.syncBusy === next.syncBusy &&
    previous.searchQuery === next.searchQuery &&
    previous.selected === next.selected &&
    previous.onSelectTask === next.onSelectTask &&
    previous.onMoveTaskToStatus === next.onMoveTaskToStatus &&
    previous.onRestoreTask === next.onRestoreTask
  );
});
