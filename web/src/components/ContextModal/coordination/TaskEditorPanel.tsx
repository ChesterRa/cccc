import {
  useEffect,
  useId,
  useRef,
  type Dispatch,
  type ReactNode,
  type RefObject,
  type SetStateAction,
} from "react";
import { Plus, X } from "lucide-react";
import type { Task, TaskChecklistItem, TaskWaitingOn } from "../../../types";
import { formatFullTime, formatTime } from "../../../utils/time";
import { classNames } from "../../../utils/classNames";
import {
  type TaskAttemptVerdict,
  type TaskTypeDefinition,
  type TaskWorkflowCoverage,
  type TaskTypeId,
} from "../../../utils/taskWorkflow";
import { SelectCombobox } from "../../SelectCombobox";
import {
  alignTaskDraftTaskType,
  getWaitingOnOptions,
  nextChecklistItemId,
  priorityLabel,
  taskOptionLabel,
  type ContextTranslator,
  type TaskDeleteInfo,
  type TaskDraft,
  type TaskStale,
} from "../model";
import type { ContextModalUi } from "../ui";
import { TaskSubtaskList } from "./TaskSubtaskList";

/**
 * Select values for a field that can be empty. An item needs a value, and the
 * field's own text, which can be any text, is kept apart from "none" by a prefix.
 */
const NONE = "none";
const asOption = (value: string) => (value ? `=${value}` : NONE);
const fromOption = (option: string) => (option.startsWith("=") ? option.slice(1) : "");
const PRIORITIES = ["low", "normal", "high", "urgent"];

interface TaskEditorPanelProps {
  tr: ContextTranslator;
  ui: ContextModalUi;
  taskEditorMode: "none" | "create" | "edit";
  taskDraft: TaskDraft | null;
  hasTaskUnsaved: boolean;
  /** A save is under way: the draft is what is being saved, so it holds still. */
  syncBusy: boolean;
  error: string;
  /** The task was deleted while it was open. */
  taskGone: boolean;
  taskStale: TaskStale;
  selectedTask: Task | null;
  subtasks: readonly Task[];
  assigneeChoices: readonly string[];
  parentChoices: readonly Task[];
  selectedTaskDeleteInfo: TaskDeleteInfo;
  selectedTaskDeleteHint: string;
  taskWorkflowCoverage: TaskWorkflowCoverage;
  taskTypeId: TaskTypeId;
  selectedTaskType: TaskTypeDefinition;
  titleRef?: RefObject<HTMLInputElement | null>;
  setTaskDraft: Dispatch<SetStateAction<TaskDraft | null>>;
  onTaskTypeChange: (value: TaskTypeId) => void;
  /** Drop the edits: a new task starts over, an existing one loads its latest version. */
  onResetTask: () => void;
  onClose: () => void;
  onDeleteSelectedTask: () => void;
  onSaveTask: () => void;
  onOpenTask: (taskId: string) => void;
  onAddSubtask: () => void;
}

/** The choices for an actor field: none, the given ones, and a value no longer among them. */
function actorItems(none: string, choices: readonly string[], value: string) {
  const ids = [...new Set([...choices, value].filter(Boolean))];
  return [{ value: NONE, label: none }, ...ids.map((id) => ({ value: asOption(id), label: id }))];
}

export function TaskEditorPanel({
  tr,
  ui,
  taskEditorMode,
  taskDraft,
  hasTaskUnsaved,
  syncBusy,
  error,
  taskGone,
  taskStale,
  selectedTask,
  subtasks,
  assigneeChoices,
  parentChoices,
  selectedTaskDeleteInfo,
  selectedTaskDeleteHint,
  taskWorkflowCoverage,
  taskTypeId,
  selectedTaskType,
  titleRef,
  setTaskDraft,
  onTaskTypeChange,
  onResetTask,
  onClose,
  onDeleteSelectedTask,
  onSaveTask,
  onOpenTask,
  onAddSubtask,
}: TaskEditorPanelProps) {
  if (taskEditorMode === "none" || !taskDraft) {
    return null;
  }

  const isCreate = taskEditorMode === "create";
  const locked = syncBusy;
  const update = (patch: Partial<TaskDraft>) =>
    setTaskDraft((prev) => (prev ? { ...prev, ...patch } : prev));
  const labelClass = classNames(
    "mb-1 block text-xs font-medium uppercase tracking-wide",
    ui.mutedTextClass,
  );
  const field = (label: string, control: ReactNode, note?: ReactNode) => (
    <label className="block min-w-0 text-sm">
      <span className={labelClass}>{label}</span>
      {control}
      {note}
    </label>
  );

  const priorityValues = [
    ...PRIORITIES,
    ...(taskDraft.priority && !PRIORITIES.includes(taskDraft.priority) ? [taskDraft.priority] : []),
  ];
  const parentItems = [
    { value: NONE, label: tr("context.rootTask", "Root task") },
    ...parentChoices
      .filter((task) => task.id !== selectedTask?.id)
      .map((task) => ({ value: asOption(task.id), label: taskOptionLabel(task) })),
    ...(taskDraft.parentId && !parentChoices.some((task) => task.id === taskDraft.parentId)
      ? [{ value: asOption(taskDraft.parentId), label: taskDraft.parentId }]
      : []),
  ];
  const heading = isCreate
    ? taskDraft.parentId
      ? tr("context.newSubtask", "New subtask of {{id}}", { id: taskDraft.parentId })
      : tr("context.newTask", "New task")
    : selectedTask?.id || tr("context.taskDetails", "Task editor");
  const isDone = String(taskDraft.status || "").toLowerCase() === "done";

  return (
    <section className="flex h-full min-h-0 flex-col" aria-busy={syncBusy}>
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5">
        <div className="flex min-w-0 items-center gap-2">
          <h2 className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
            {heading}
          </h2>
          {hasTaskUnsaved ? (
            <span className="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] font-medium text-amber-600 dark:text-amber-400">
              {tr("context.unsaved", "Unsaved")}
            </span>
          ) : null}
        </div>
        <button type="button" onClick={onClose} className={ui.buttonSecondaryClass}>
          {tr("context.close", "Close")}
        </button>
      </div>

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto overscroll-contain px-4 py-4 sm:px-5">
        {error ? (
          <div
            role="alert"
            className="rounded-xl border border-rose-500/30 bg-rose-500/15 px-3 py-2 text-sm text-rose-600 dark:text-rose-400"
          >
            {error}
          </div>
        ) : null}
        {taskGone ? (
          <div
            role="alert"
            className="rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-300"
          >
            {tr(
              "context.taskGone",
              "This task no longer exists, so your edits can't be saved here. Copy anything you want to keep.",
            )}
          </div>
        ) : taskStale ? (
          <div
            role="alert"
            className="space-y-2 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-300"
          >
            <p>
              {taskStale === "unconfirmed"
                ? tr(
                    "context.taskReadbackFailed",
                    "Saved, but the latest version could not be loaded. Reload it before editing again.",
                  )
                : tr(
                    "context.taskChangedElsewhere",
                    "This task has changed since you opened it. Saving now could overwrite those changes, so it is refused. Copy anything you want to keep, then reload the latest version.",
                  )}
            </p>
            <button
              type="button"
              onClick={onResetTask}
              disabled={syncBusy}
              className={ui.buttonSecondaryClass}
            >
              {tr("context.reloadLatest", "Reload latest version")}
            </button>
          </div>
        ) : null}

        {field(
          tr("context.titleField", "Title"),
          <input
            ref={titleRef}
            value={taskDraft.title}
            readOnly={locked}
            onChange={(event) => update({ title: event.target.value })}
            className={classNames(ui.inputClass, "text-base font-semibold")}
          />,
        )}

        <div className="grid gap-3 sm:grid-cols-2">
          {field(
            tr("context.status", "Status"),
            <SelectCombobox
              items={[
                { value: "planned", label: tr("context.planned", "Planned") },
                { value: "active", label: tr("context.active", "Active") },
                { value: "done", label: tr("context.done", "Done") },
                { value: "archived", label: tr("context.archived", "Archived") },
              ]}
              value={taskDraft.status}
              onChange={(value) => update({ status: value })}
              ariaLabel={tr("context.status", "Status")}
              className={ui.inputClass}
              disabled={locked}
            />,
          )}
          {field(
            tr("context.assignee", "Assignee"),
            <SelectCombobox
              items={actorItems(
                tr("context.unassigned", "Unassigned"),
                assigneeChoices,
                taskDraft.assignee,
              )}
              value={asOption(taskDraft.assignee)}
              onChange={(value) => update({ assignee: fromOption(value) })}
              ariaLabel={tr("context.assignee", "Assignee")}
              className={ui.inputClass}
              disabled={locked}
              searchable
            />,
          )}
          {field(
            tr("context.priority", "Priority"),
            <SelectCombobox
              items={[
                { value: NONE, label: tr("context.noPriority", "No priority") },
                ...priorityValues.map((value) => ({
                  value: asOption(value),
                  label: priorityLabel(value, tr),
                })),
              ]}
              value={asOption(taskDraft.priority)}
              onChange={(value) => update({ priority: fromOption(value) })}
              ariaLabel={tr("context.priority", "Priority")}
              className={ui.inputClass}
              disabled={locked}
            />,
          )}
          {field(
            tr("context.waitingOn", "Waiting on"),
            <SelectCombobox
              items={getWaitingOnOptions(tr)}
              value={taskDraft.waitingOn}
              onChange={(value) => update({ waitingOn: value as TaskWaitingOn })}
              ariaLabel={tr("context.waitingOn", "Waiting on")}
              className={ui.inputClass}
              disabled={locked}
            />,
          )}
        </div>

        {field(
          tr("context.outcome", "Outcome"),
          <textarea
            value={taskDraft.outcome}
            readOnly={locked}
            onChange={(event) => update({ outcome: event.target.value })}
            placeholder={tr("context.outcomePlaceholder", "The result, in a sentence or two")}
            className={classNames(ui.textareaClass, "min-h-[72px]")}
          />,
          isDone && !taskDraft.outcome.trim() ? (
            <span className={classNames("mt-1 block text-xs", ui.mutedTextClass)}>
              {tr("context.outcomeDoneHint", "Add a short result so others can see what was done.")}
            </span>
          ) : null,
        )}

        <ChecklistEditor
          tr={tr}
          ui={ui}
          items={taskDraft.checklist}
          locked={locked}
          onChange={(checklist) => update({ checklist })}
        />

        {field(
          tr("context.notes", "Notes"),
          <textarea
            value={taskDraft.notes}
            readOnly={locked}
            onChange={(event) => update({ notes: event.target.value })}
            className={classNames(ui.textareaClass, "min-h-[180px]")}
          />,
        )}

        {isCreate ? null : (
          <TaskSubtaskList
            tr={tr}
            ui={ui}
            subtasks={subtasks}
            onOpenTask={onOpenTask}
            onAddSubtask={taskGone ? undefined : onAddSubtask}
          />
        )}

        <details className="rounded-xl border border-[var(--glass-border-subtle)] px-3 py-3">
          <summary className="cursor-pointer text-sm font-medium text-[var(--color-text-primary)]">
            {tr("context.advancedTaskFields", "Advanced details")}
          </summary>
          <div className="mt-3 space-y-3">
            <div className="grid gap-3 sm:grid-cols-2">
              {field(
                tr("context.parentTask", "Parent task"),
                <SelectCombobox
                  items={parentItems}
                  value={asOption(taskDraft.parentId)}
                  onChange={(value) => {
                    const parentId = fromOption(value);
                    setTaskDraft((prev) =>
                      prev
                        ? {
                            ...prev,
                            parentId,
                            taskType: alignTaskDraftTaskType(
                              prev.taskType,
                              parentId,
                              prev.parentId,
                            ),
                          }
                        : prev,
                    );
                  }}
                  ariaLabel={tr("context.parentTask", "Parent task")}
                  className={ui.inputClass}
                  disabled={locked}
                  searchable
                />,
              )}
              {field(
                tr("context.handoffField", "Hand off to"),
                <SelectCombobox
                  items={actorItems(
                    tr("context.none", "None"),
                    assigneeChoices,
                    taskDraft.handoffTo,
                  )}
                  value={asOption(taskDraft.handoffTo)}
                  onChange={(value) => update({ handoffTo: fromOption(value) })}
                  ariaLabel={tr("context.handoffField", "Hand off to")}
                  className={ui.inputClass}
                  disabled={locked}
                  searchable
                />,
              )}
            </div>
            {field(
              tr("context.blockedBy", "Blocked by"),
              <textarea
                value={taskDraft.blockedBy}
                readOnly={locked}
                onChange={(event) => update({ blockedBy: event.target.value })}
                className={classNames(ui.textareaClass, "min-h-[72px]")}
                placeholder={tr("context.blockedByPlaceholder", "Task IDs, one per line")}
              />,
            )}
            <TaskTypeSection
              tr={tr}
              ui={ui}
              taskDraft={taskDraft}
              taskTypeId={taskTypeId}
              selectedTaskType={selectedTaskType}
              taskWorkflowCoverage={taskWorkflowCoverage}
              locked={locked}
              onTaskTypeChange={onTaskTypeChange}
            />
          </div>
        </details>

        {!isCreate && selectedTask ? (
          <p className={classNames("text-xs", ui.mutedTextClass)}>
            {[
              selectedTask.created_at
                ? `${tr("context.createdAt", "Created")} ${formatFullTime(selectedTask.created_at)}`
                : "",
              selectedTask.updated_at
                ? tr("context.updated", "Updated {{time}}", {
                    time: formatTime(selectedTask.updated_at),
                  })
                : "",
              selectedTask.archived_from
                ? `${tr("context.archivedFrom", "Archived from")} ${selectedTask.archived_from}`
                : "",
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
        ) : null}
        {/* Why Delete is not offered: read when looked for, not kept on screen. */}
        {!isCreate && selectedTask && selectedTaskDeleteHint ? (
          <p className={classNames("text-xs", ui.mutedTextClass)}>{selectedTaskDeleteHint}</p>
        ) : null}
      </div>

      <div className="shrink-0 border-t border-[var(--glass-border-subtle)] px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] sm:px-5">
        <div className="flex flex-wrap items-center gap-2">
          {!isCreate && selectedTask && selectedTaskDeleteInfo.allowed ? (
            <button
              type="button"
              onClick={onDeleteSelectedTask}
              disabled={syncBusy}
              className={ui.buttonDangerClass}
            >
              {tr("context.deleteTask", "Delete")}
            </button>
          ) : null}
          <div className="ml-auto flex flex-wrap items-center gap-2">
            {hasTaskUnsaved && !taskGone ? (
              <button
                type="button"
                onClick={onResetTask}
                disabled={syncBusy}
                className={ui.buttonSecondaryClass}
              >
                {isCreate
                  ? tr("context.clear", "Clear")
                  : tr("context.discardChanges", "Discard changes")}
              </button>
            ) : null}
            <button
              type="button"
              onClick={onSaveTask}
              disabled={
                syncBusy ||
                taskGone ||
                taskStale === "unconfirmed" ||
                (!isCreate && !hasTaskUnsaved)
              }
              className={ui.buttonPrimaryClass}
            >
              {syncBusy
                ? tr("context.saving", "Saving…")
                : isCreate
                  ? tr("context.createTask", "Create task")
                  : tr("context.saveTask", "Save task")}
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}

function ChecklistEditor({
  tr,
  ui,
  items,
  locked,
  onChange,
}: {
  tr: ContextTranslator;
  ui: ContextModalUi;
  items: TaskChecklistItem[];
  locked: boolean;
  onChange: (items: TaskChecklistItem[]) => void;
}) {
  const inputs = useRef(new Map<string, HTMLInputElement>());
  const focusId = useRef("");
  const statusId = useId();
  useEffect(() => {
    if (!focusId.current) return;
    inputs.current.get(focusId.current)?.focus();
    focusId.current = "";
  });
  const add = (after = items.length - 1) => {
    const id = nextChecklistItemId(items);
    focusId.current = id;
    onChange([
      ...items.slice(0, after + 1),
      { id, text: "", status: "pending" },
      ...items.slice(after + 1),
    ]);
  };
  const change = (index: number, patch: Partial<TaskChecklistItem>) =>
    onChange(items.map((item, at) => (at === index ? { ...item, ...patch } : item)));
  const remove = (index: number) => {
    focusId.current = items[index - 1]?.id || "";
    onChange(items.filter((_, at) => at !== index));
  };
  const listed = items.filter((item) => item.text.trim());
  const done = listed.filter((item) => item.status === "done").length;

  return (
    <fieldset className="min-w-0">
      <legend className="mb-1 flex w-full items-center justify-between gap-2">
        <span
          className={classNames("text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}
        >
          {tr("context.checklist", "Checklist")}
        </span>
        {listed.length > 0 ? (
          <span className={classNames("text-xs", ui.mutedTextClass)}>
            {tr("context.checklistDone", "{{done}} of {{total}} done", {
              done,
              total: listed.length,
            })}
          </span>
        ) : null}
      </legend>
      {items.length > 0 ? (
        <ul className="space-y-1.5">
          {items.map((item, index) => {
            const label = tr("context.checklistItem", "Checklist item {{n}}", { n: index + 1 });
            // A status the checkbox cannot show is shown as written beside it.
            const other =
              item.status && !["pending", "in_progress", "done"].includes(item.status)
                ? String(item.status)
                : "";
            return (
              <li key={`${item.id}:${index}`} className="flex items-center gap-1">
                <label
                  className="flex size-10 shrink-0 cursor-pointer items-center justify-center"
                  title={
                    item.status === "in_progress"
                      ? tr("context.checklistInProgress", "In progress")
                      : undefined
                  }
                >
                  <input
                    type="checkbox"
                    className="size-4 cursor-pointer accent-[var(--color-text-primary)]"
                    checked={item.status === "done"}
                    ref={(element) => {
                      if (element) element.indeterminate = item.status === "in_progress";
                    }}
                    disabled={locked}
                    onChange={() =>
                      change(index, { status: item.status === "done" ? "pending" : "done" })
                    }
                    aria-label={tr("context.checklistItemDone", "Done: {{item}}", {
                      item: item.text.trim() || label,
                    })}
                    aria-describedby={other ? `${statusId}-${index}` : undefined}
                  />
                </label>
                <input
                  ref={(element) => {
                    if (element) inputs.current.set(item.id, element);
                    else inputs.current.delete(item.id);
                  }}
                  value={item.text}
                  readOnly={locked}
                  aria-label={label}
                  onChange={(event) => change(index, { text: event.target.value })}
                  onKeyDown={(event) => {
                    // Enter that ends an IME composition only commits the text.
                    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
                    if (event.key === "Enter") {
                      event.preventDefault();
                      if (item.text.trim()) add(index);
                    } else if (event.key === "Backspace" && !item.text) {
                      event.preventDefault();
                      remove(index);
                    }
                  }}
                  className={classNames(
                    ui.inputClass,
                    "min-h-10 min-w-0 flex-1 px-3 py-2",
                    item.status === "done" && "text-[var(--color-text-muted)] line-through",
                  )}
                />
                {other ? (
                  <span
                    id={`${statusId}-${index}`}
                    title={other}
                    className="max-w-[6rem] shrink-0 truncate rounded-full border border-[var(--glass-border-subtle)] px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)]"
                  >
                    {other}
                  </span>
                ) : null}
                <button
                  type="button"
                  disabled={locked}
                  onClick={() => remove(index)}
                  aria-label={tr("context.checklistRemove", "Remove item {{n}}", { n: index + 1 })}
                  className="flex size-10 shrink-0 items-center justify-center rounded-lg text-[var(--color-text-muted)] hover:bg-[var(--glass-tab-bg)] hover:text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] disabled:opacity-50"
                >
                  <X className="size-4" aria-hidden="true" />
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
      <button
        type="button"
        disabled={locked}
        onClick={() => add()}
        className={classNames(ui.buttonSecondaryClass, "mt-2 gap-1.5")}
      >
        <Plus className="size-4" aria-hidden="true" />
        {tr("context.checklistAdd", "Add item")}
      </button>
    </fieldset>
  );
}

/**
 * The task type and what it asks of a task's notes. These describe how
 * complete a record is; they never decide whether it can move or finish.
 */
function TaskTypeSection({
  tr,
  ui,
  taskDraft,
  taskTypeId,
  selectedTaskType,
  taskWorkflowCoverage,
  locked,
  onTaskTypeChange,
}: {
  tr: ContextTranslator;
  ui: ContextModalUi;
  taskDraft: TaskDraft;
  taskTypeId: TaskTypeId;
  selectedTaskType: TaskTypeDefinition;
  taskWorkflowCoverage: TaskWorkflowCoverage;
  locked: boolean;
  onTaskTypeChange: (value: TaskTypeId) => void;
}) {
  const attemptToneClass = (verdict: TaskAttemptVerdict) =>
    classNames(
      "rounded-full px-2 py-0.5 text-[11px] font-medium",
      verdict === "keep"
        ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400"
        : verdict === "discard"
          ? "bg-amber-500/15 text-amber-600 dark:text-amber-400"
          : verdict === "crash"
            ? "bg-rose-500/15 text-rose-600 dark:text-rose-400"
            : "glass-panel text-[var(--color-text-secondary)]",
    );
  const presentClass = (ok: boolean) =>
    classNames(
      "rounded-full px-2 py-0.5 text-[11px] font-medium",
      ok
        ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400"
        : "glass-panel text-[var(--color-text-secondary)]",
    );
  const latestAttemptLabel =
    taskWorkflowCoverage.latestAttemptVerdict === "keep"
      ? tr("context.latestAttemptKeep", "Latest keep")
      : taskWorkflowCoverage.latestAttemptVerdict === "discard"
        ? tr("context.latestAttemptDiscard", "Latest discard")
        : taskWorkflowCoverage.latestAttemptVerdict === "crash"
          ? tr("context.latestAttemptCrash", "Latest crash")
          : taskWorkflowCoverage.latestAttemptVerdict === "continue"
            ? tr("context.latestAttemptContinue", "Latest continue")
            : "";
  const taskTypeDescription =
    selectedTaskType.id === "free"
      ? tr(
          "context.taskTypeDescFree",
          "Keep it lightweight. Use this when extra structure would add more ceremony than control.",
        )
      : selectedTaskType.id === "standard"
        ? tr(
            "context.taskTypeDescStandard",
            "Use the normal closed-loop contract: goal, evidence, owner, and closeout.",
          )
        : tr(
            "context.taskTypeDescOptimization",
            "For metric-sensitive work. Capture baseline, metric, verifier boundary, current best, and next frontier.",
          );
  const structured =
    taskWorkflowCoverage.taskTypeFamily === "standard" || taskWorkflowCoverage.isOptimization;
  const isDone = String(taskDraft.status || "").toLowerCase() === "done";

  return (
    <div className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-3">
      <label className="block text-sm">
        <span
          className={classNames(
            "mb-1 block text-xs font-medium uppercase tracking-wide",
            ui.mutedTextClass,
          )}
        >
          {tr("context.taskWorkflow", "Task type")}
        </span>
        <SelectCombobox
          items={[
            { value: "free", label: tr("context.taskTypeFree", "Free") },
            { value: "standard", label: tr("context.taskTypeStandard", "Standard") },
            { value: "optimization", label: tr("context.taskTypeOptimization", "Optimization") },
          ]}
          value={taskTypeId}
          onChange={(value) => onTaskTypeChange(value as TaskTypeId)}
          ariaLabel={tr("context.taskType", "Task type")}
          className={ui.inputClass}
          disabled={locked}
        />
      </label>
      <p className={classNames("text-xs", ui.mutedTextClass)}>{taskTypeDescription}</p>
      {structured ? (
        <div className="flex flex-wrap gap-1.5">
          <span className={presentClass(taskWorkflowCoverage.hasGoal)}>
            {tr("context.goal", "Goal")}
          </span>
          <span className={presentClass(taskWorkflowCoverage.hasSuccessCriteria)}>
            {tr("context.successCriteria", "Success criteria")}
          </span>
          <span className={presentClass(taskWorkflowCoverage.hasRequiredEvidence)}>
            {tr("context.requiredEvidence", "Required evidence")}
          </span>
          {taskWorkflowCoverage.taskTypeFamily === "standard" ? (
            <span className={presentClass(taskWorkflowCoverage.hasOwner)}>
              {tr("context.owner", "Owner")}
            </span>
          ) : null}
          {taskWorkflowCoverage.isOptimization ? (
            <>
              <span className={presentClass(taskWorkflowCoverage.hasBaseline)}>
                {tr("context.baseline", "Baseline")}
              </span>
              <span className={presentClass(taskWorkflowCoverage.hasPrimaryMetric)}>
                {tr("context.primaryMetric", "Primary metric")}
              </span>
              <span className={presentClass(taskWorkflowCoverage.hasVerifierBoundary)}>
                {tr("context.verifierBoundary", "Verifier boundary")}
              </span>
              <span className={presentClass(taskWorkflowCoverage.hasCurrentBest)}>
                {tr("context.currentBest", "Current best")}
              </span>
              <span className={presentClass(taskWorkflowCoverage.hasFrontierNext)}>
                {tr("context.frontierNext", "Frontier next")}
              </span>
            </>
          ) : null}
          {isDone ? (
            <>
              <span className={presentClass(taskWorkflowCoverage.hasCloseoutVerdict)}>
                {tr("context.closeoutVerdict", "Closeout verdict")}
              </span>
              <span className={presentClass(taskWorkflowCoverage.hasVerificationSummary)}>
                {tr("context.verificationSummary", "Verification summary")}
              </span>
              {taskWorkflowCoverage.isOptimization ? (
                <span className={presentClass(taskWorkflowCoverage.hasAttemptDecision)}>
                  {tr("context.attemptDecision", "Attempt decision")}
                </span>
              ) : null}
            </>
          ) : null}
        </div>
      ) : null}
      {taskWorkflowCoverage.isOptimization &&
      (taskWorkflowCoverage.hasCurrentBest ||
        taskWorkflowCoverage.hasFrontierNext ||
        taskWorkflowCoverage.hasAttemptLog) ? (
        <div className="grid gap-2 text-xs">
          {taskWorkflowCoverage.hasCurrentBest ? (
            <div>
              <div className={classNames("font-medium", ui.mutedTextClass)}>
                {tr("context.currentBest", "Current best")}
              </div>
              <div className="mt-1 text-[var(--color-text-primary)]">
                {taskWorkflowCoverage.currentBestSummary}
              </div>
            </div>
          ) : null}
          {taskWorkflowCoverage.hasFrontierNext ? (
            <div>
              <div className={classNames("font-medium", ui.mutedTextClass)}>
                {tr("context.frontierNext", "Frontier next")}
              </div>
              <div className="mt-1 text-[var(--color-text-primary)]">
                {taskWorkflowCoverage.frontierNextSummary}
              </div>
            </div>
          ) : null}
          {taskWorkflowCoverage.hasAttemptLog ? (
            <div className="flex flex-wrap items-center gap-2">
              {latestAttemptLabel ? (
                <span className={attemptToneClass(taskWorkflowCoverage.latestAttemptVerdict)}>
                  {latestAttemptLabel}
                </span>
              ) : null}
              <span className={classNames("min-w-0 flex-1", ui.subtleTextClass)}>
                {taskWorkflowCoverage.latestAttemptSummary ||
                  tr("context.attemptLogPresent", "Attempt log recorded")}
              </span>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
