import { DndContext, DragOverlay, type DragEndEvent, type DragStartEvent } from "@dnd-kit/core";
import { useDraggable, useDroppable } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import type { SensorDescriptor, SensorOptions } from "@dnd-kit/core";
import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { evaluateTaskWorkflow, type TaskAttemptVerdict } from "../../../utils/taskWorkflow";
import { SelectCombobox } from "../../SelectCombobox";
import {
  statusTone,
  taskDisplaySummary,
  taskStatus,
  taskTitle,
  waitingLabel,
  type BoardColumns,
  type BoardStatus,
  type ContextTranslator,
  type TaskFilterValue,
} from "../model";
import type { ContextModalUi } from "../ui";

interface AttentionCounts {
  blocked: number;
  waitingUser: number;
  pendingHandoffs: number;
}

interface TaskBoardProps {
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  taskQuery: string;
  assigneeFilter: string;
  assigneeOptions: string[];
  taskFilter: TaskFilterValue;
  tasksSummary: {
    total?: number;
    archived?: number;
  };
  attentionCounts: AttentionCounts;
  unassignedCount: number;
  hasArchivedTasks: boolean;
  archivedExpanded: boolean;
  hasVisibleTasks: boolean;
  hiddenArchivedMatches: number;
  filteredBoard: BoardColumns;
  taskMap: Map<string, Task>;
  selectedTaskId: string;
  dragTaskId: string;
  sensors: SensorDescriptor<SensorOptions>[];
  onTaskQueryChange: (value: string) => void;
  onAssigneeFilterChange: (value: string) => void;
  onTaskFilterChange: (value: TaskFilterValue) => void;
  onClearFilters: () => void;
  onArchivedExpandedChange: (value: boolean) => void;
  onOpenCreate: (status?: BoardStatus) => void;
  onDragStart: (event: DragStartEvent) => void;
  onDragEnd: (event: DragEndEvent) => void;
  onDragCancel: () => void;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
}

function latestAttemptTone(verdict: TaskAttemptVerdict): string {
  if (verdict === "keep") return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400";
  if (verdict === "discard") return "bg-amber-500/15 text-amber-600 dark:text-amber-400";
  if (verdict === "crash") return "bg-rose-500/15 text-rose-600 dark:text-rose-400";
  return "glass-panel text-[var(--color-text-secondary)]";
}

function latestAttemptLabel(verdict: TaskAttemptVerdict, tr: ContextTranslator): string {
  if (verdict === "keep") return tr("context.latestAttemptKeep", "Latest keep");
  if (verdict === "discard") return tr("context.latestAttemptDiscard", "Latest discard");
  if (verdict === "crash") return tr("context.latestAttemptCrash", "Latest crash");
  if (verdict === "continue") return tr("context.latestAttemptContinue", "Latest continue");
  return "";
}

function showMissingCurrentBest(workflow: ReturnType<typeof evaluateTaskWorkflow>): boolean {
  return workflow.isOptimization && !workflow.needsContract && !workflow.hasCurrentBest;
}

function TaskGhostCard({
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
  const blocked = Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
  const workflow = evaluateTaskWorkflow({
    parent_id: task.parent_id,
    task_type: task.task_type,
    status: task.status,
    assignee: task.assignee,
    outcome: task.outcome,
    notes: task.notes,
    checklist: task.checklist,
  });

  return (
    <div className={classNames(
      "w-[320px] rounded-2xl border p-3 shadow-2xl",
      "glass-panel"
    )}>
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className={classNames("truncate text-sm font-semibold", "text-[var(--color-text-primary)]")}>{taskTitle(task)}</div>
          <div className={classNames("mt-1 text-xs", mutedTextClass)}>{task.id}</div>
        </div>
        <span className={classNames("rounded-full border px-2 py-0.5 text-[11px] font-medium", statusTone(status))}>{status}</span>
      </div>
      {taskDisplaySummary(task) ? <div className={classNames("mt-2 line-clamp-3 text-xs", subtleTextClass)}>{taskDisplaySummary(task)}</div> : null}
      <div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">
        {task.assignee ? <span className={classNames("rounded-full px-2 py-0.5", "glass-panel text-[var(--color-text-secondary)]")}>{task.assignee}</span> : null}
        {blocked ? <span className={classNames("rounded-full px-2 py-0.5", "bg-rose-500/15 text-rose-600 dark:text-rose-400")}>{tr("context.blocked", "Blocked")}</span> : null}
        {workflow.isOptimization && workflow.latestAttemptVerdict ? (
          <span className={classNames("rounded-full px-2 py-0.5", latestAttemptTone(workflow.latestAttemptVerdict))}>
            {latestAttemptLabel(workflow.latestAttemptVerdict, tr)}
          </span>
        ) : null}
        {showMissingCurrentBest(workflow) ? (
          <span className={classNames("rounded-full px-2 py-0.5", "bg-amber-500/12 text-amber-700 dark:text-amber-300")}>
            {tr("context.noCurrentBestYet", "No best yet")}
          </span>
        ) : null}
        {workflow.needsContract ? <span className={classNames("rounded-full px-2 py-0.5", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>{tr("context.needsContract", "Needs requirements")}</span> : null}
        {workflow.needsCloseout ? <span className={classNames("rounded-full px-2 py-0.5", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>{tr("context.needsCloseout", "Needs closeout")}</span> : null}
      </div>
    </div>
  );
}

function TaskCard({
  task,
  tr,
  ui,
  syncBusy,
  selectedTaskId,
  onSelectTask,
  onMoveTaskToStatus,
}: {
  task: Task;
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  selectedTaskId: string;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
}) {
  const status = taskStatus(task);
  const blocked = Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
  const waiting = String(task.waiting_on || "none").trim();
  const handoff = String(task.handoff_to || "").trim();
  const workflow = evaluateTaskWorkflow({
    parent_id: task.parent_id,
    task_type: task.task_type,
    status: task.status,
    assignee: task.assignee,
    outcome: task.outcome,
    notes: task.notes,
    checklist: task.checklist,
  });
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: `task:${task.id}`,
    disabled: syncBusy,
    data: { type: "task", taskId: task.id, status },
  });
  const style = { transform: CSS.Translate.toString(transform) };
  const quickAction = status === "planned" ? { label: tr("context.start", "Start"), next: "active" as BoardStatus }
    : status === "active" ? { label: tr("context.done", "Done"), next: "done" as BoardStatus }
    : status === "done" ? { label: tr("context.reopen", "Reopen"), next: "active" as BoardStatus }
    : { label: tr("context.restore", "Restore"), next: "planned" as BoardStatus };

  return (
    <div ref={setNodeRef} style={style} className={classNames("group/task", isDragging && "z-20 opacity-80")}>
      <div
        id={`context-task-${task.id}`}
        data-task-id={task.id}
        {...attributes}
        onClick={() => onSelectTask(task)}
        className={classNames(
          "w-full cursor-pointer rounded-2xl border p-3 text-left transition-all",
          blocked
            ? "border-rose-500/30 bg-rose-500/5"
            : selectedTaskId === task.id
              ? "border-black/10 bg-[rgb(245,245,245)] shadow-[0_0_0_1px_rgba(17,24,39,0.08)] dark:border-white/12 dark:bg-white/[0.08] dark:shadow-[0_0_0_1px_rgba(255,255,255,0.06)]"
              : "glass-panel hover:border-[var(--glass-border-subtle)]"
        )}
      >
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1 text-left">
            <div className={classNames("truncate text-sm font-semibold", "text-[var(--color-text-primary)]")}>{taskTitle(task)}</div>
            <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{task.id}</div>
          </div>
          <div className="flex items-center gap-2">
            <span className={classNames("rounded-full border px-2 py-0.5 text-[11px] font-medium", statusTone(status))}>{status}</span>
            <button
              type="button"
              {...listeners}
              className={classNames(
                "rounded-lg px-2 py-1 text-[11px] md:opacity-0 md:group-hover/task:opacity-100",
                "glass-btn text-[var(--color-text-secondary)]"
              )}
              onClick={(event) => event.stopPropagation()}
              aria-label={tr("context.dragTask", "Drag task")}
              title={tr("context.dragTask", "Drag task")}
            >
              ⋮⋮
            </button>
          </div>
        </div>

        {taskDisplaySummary(task) ? (
          <div className={classNames("mt-2 block w-full text-left line-clamp-3 text-xs", ui.subtleTextClass)}>{taskDisplaySummary(task)}</div>
        ) : null}

        <div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">
          {task.assignee ? (
            <span className={classNames("rounded-full px-2 py-0.5", "glass-panel text-[var(--color-text-secondary)]")}>{task.assignee}</span>
          ) : (
            <span className={classNames("rounded-full px-2 py-0.5", "bg-[var(--glass-tab-bg)] text-[var(--color-text-muted)]")}>{tr("context.unassigned", "Unassigned")}</span>
          )}
          {task.priority ? <span className={classNames("rounded-full px-2 py-0.5", "glass-panel text-[var(--color-text-secondary)]")}>{task.priority}</span> : null}
          {blocked ? <span className={classNames("rounded-full px-2 py-0.5", "bg-rose-500/15 text-rose-600 dark:text-rose-400")}>{tr("context.blocked", "Blocked")}</span> : null}
          {waiting && waiting !== "none" ? <span className={classNames("rounded-full px-2 py-0.5", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>{waitingLabel(waiting, tr)}</span> : null}
          {handoff ? <span className={classNames("rounded-full px-2 py-0.5", "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white")}>{tr("context.handoffTo", "Handoff →")} {handoff}</span> : null}
          {workflow.isOptimization && workflow.latestAttemptVerdict ? (
            <span className={classNames("rounded-full px-2 py-0.5", latestAttemptTone(workflow.latestAttemptVerdict))}>
              {latestAttemptLabel(workflow.latestAttemptVerdict, tr)}
            </span>
          ) : null}
          {showMissingCurrentBest(workflow) ? (
            <span className={classNames("rounded-full px-2 py-0.5", "bg-amber-500/12 text-amber-700 dark:text-amber-300")}>
              {tr("context.noCurrentBestYet", "No best yet")}
            </span>
          ) : null}
          {workflow.needsContract ? <span className={classNames("rounded-full px-2 py-0.5", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>{tr("context.needsContract", "Needs requirements")}</span> : null}
          {workflow.needsCloseout ? <span className={classNames("rounded-full px-2 py-0.5", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>{tr("context.needsCloseout", "Needs closeout")}</span> : null}
        </div>

        <div className="mt-3 flex items-center gap-2 border-t pt-3" onClick={(event) => event.stopPropagation()}>
          <button type="button" onClick={() => onMoveTaskToStatus(task, quickAction.next)} disabled={syncBusy} className={ui.buttonSecondaryClass}>{quickAction.label}</button>
          <button type="button" onClick={() => onSelectTask(task)} className={ui.buttonSecondaryClass}>{tr("context.edit", "Edit")}</button>
        </div>
      </div>
    </div>
  );
}

function ColumnDropZone({
  columnKey,
  label,
  items,
  tr,
  ui,
  syncBusy,
  selectedTaskId,
  onSelectTask,
  onMoveTaskToStatus,
}: {
  columnKey: BoardStatus;
  label: string;
  items: Task[];
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  selectedTaskId: string;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `column:${columnKey}`, data: { type: "column", status: columnKey } });

  return (
    <section ref={setNodeRef} className={classNames(
      "rounded-2xl border p-3 transition-all",
      isOver
        ? "border-black/10 bg-[rgb(245,245,245)] shadow-[0_0_0_1px_rgba(17,24,39,0.08)] dark:border-white/12 dark:bg-white/[0.08] dark:shadow-[0_0_0_1px_rgba(255,255,255,0.06)]"
        : "glass-panel"
    )}>
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{label}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{items.length} {tr("context.items", "items")}</div>
        </div>
        <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", "glass-panel text-[var(--color-text-tertiary)]")}>{items.length}</span>
      </div>
      <div className="mt-3">
        <div className="space-y-2">
          {items.length > 0 ? items.map((task) => (
            <TaskCard
              key={task.id}
              task={task}
              tr={tr}
              ui={ui}
              syncBusy={syncBusy}
              selectedTaskId={selectedTaskId}
              onSelectTask={onSelectTask}
              onMoveTaskToStatus={onMoveTaskToStatus}
            />
          )) : (
            <div className={classNames("rounded-lg border border-dashed px-3 py-5 text-xs", "border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]")}>
              {tr(`context.empty.${columnKey}`, "No tasks here")}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

export function TaskBoard({
  tr,
  ui,
  syncBusy,
  taskQuery,
  assigneeFilter,
  assigneeOptions,
  taskFilter,
  tasksSummary,
  attentionCounts,
  unassignedCount,
  hasArchivedTasks,
  archivedExpanded,
  hasVisibleTasks,
  hiddenArchivedMatches,
  filteredBoard,
  taskMap,
  selectedTaskId,
  dragTaskId,
  sensors,
  onTaskQueryChange,
  onAssigneeFilterChange,
  onTaskFilterChange,
  onClearFilters,
  onArchivedExpandedChange,
  onOpenCreate,
  onDragStart,
  onDragEnd,
  onDragCancel,
  onSelectTask,
  onMoveTaskToStatus,
}: TaskBoardProps) {
  return (
    <section className={classNames(ui.surfaceClass, "p-4")}>
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
          <div>
            <div className={classNames("text-lg font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.tasks", "Tasks")}</div>
            <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>{tr("context.taskBoardHint", "Plan shared work here. Open a card only when you need blockers, handoffs, notes, or checklist detail.")}</div>
          </div>
          <div className="flex items-center gap-2">
            <button type="button" onClick={() => onOpenCreate("planned")} className={ui.buttonPrimaryClass}>{tr("context.newTask", "New task")}</button>
          </div>
        </div>

        <div className={classNames("flex flex-col gap-3 border-t pt-4", "border-[var(--glass-border-subtle)]")}>
          <div className="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
            <div className="grid flex-1 gap-3 lg:grid-cols-[minmax(0,1fr)_auto_auto]">
              <input
                value={taskQuery}
                onChange={(event) => onTaskQueryChange(event.target.value)}
                className={ui.inputClass}
                placeholder={tr("context.searchTasks", "Search tasks by title, id, assignee, or outcome")}
              />
              <SelectCombobox
                items={[
                  { value: "__all__", label: tr("context.allAssignees", "All assignees") },
                  { value: "__unassigned__", label: tr("context.unassignedOnly", "Unassigned only") },
                  ...assigneeOptions.map((assignee) => ({ value: assignee, label: assignee })),
                ]}
                value={assigneeFilter}
                onChange={onAssigneeFilterChange}
                ariaLabel={tr("context.assignee", "Assignee")}
                className={classNames(ui.inputClass, "w-full lg:w-[14rem]")}
                searchable
              />
              <button type="button" onClick={onClearFilters} className={ui.buttonSecondaryClass}>{tr("context.clearFilters", "Clear filters")}</button>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            {[
              ["all", tr("context.all", "All"), Number(tasksSummary.total || 0)],
              ["blocked", tr("context.blocked", "Blocked"), attentionCounts.blocked],
              ["waiting_user", tr("context.waitingUser", "Waiting user"), attentionCounts.waitingUser],
              ["handoff", tr("context.pendingHandoffs", "Pending handoffs"), attentionCounts.pendingHandoffs],
              ["unassigned", tr("context.unassigned", "Unassigned"), unassignedCount],
            ].map(([value, label, count]) => (
              <button
                key={String(value)}
                type="button"
                onClick={() => onTaskFilterChange(value as TaskFilterValue)}
                className={classNames(
                  ui.chipBaseClass,
                  taskFilter === value
                    ? "border-black/10 bg-[rgb(35,36,37)] text-white shadow-[0_10px_24px_-20px_rgba(15,23,42,0.3)] dark:border-white/12 dark:bg-white dark:text-[rgb(20,20,22)]"
                    : ""
                )}
              >
                {label} · {count}
              </button>
            ))}
            {hasArchivedTasks ? (
              <div className="flex items-center gap-2 sm:ml-auto">
                <span className={classNames("text-sm font-medium", "text-[var(--color-text-primary)]")}>
                  {tr("context.showArchived", "Show archived")}
                </span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={archivedExpanded}
                  aria-label={tr("context.showArchived", "Show archived")}
                  onClick={() => onArchivedExpandedChange(!archivedExpanded)}
                  className={ui.switchTrackClass(archivedExpanded)}
                >
                  <span className={ui.switchThumbClass(archivedExpanded)} />
                </button>
              </div>
            ) : null}
            {syncBusy ? <span className={classNames("text-xs italic", ui.mutedTextClass)}>{tr("context.applyingChanges", "Applying changes…")}</span> : null}
          </div>
        </div>

        {!hasVisibleTasks && hiddenArchivedMatches === 0 ? (
          <div className={classNames("rounded-xl border border-dashed px-4 py-6 text-sm", "glass-card text-[var(--color-text-muted)]")}>
            {tr("context.noMatchingTasks", "No tasks match the current filters")}
          </div>
        ) : null}

        {!hasVisibleTasks && hiddenArchivedMatches > 0 ? (
          <div className={classNames("rounded-xl border border-dashed px-4 py-5 text-sm", "glass-card text-[var(--color-text-muted)]")}>
            <div>{tr("context.archivedHiddenMatchesDetail", "{{count}} archived tasks match the current filters. Show archived to review them.", { count: hiddenArchivedMatches })}</div>
            <div className="mt-3">
              <button type="button" onClick={() => onArchivedExpandedChange(true)} className={ui.buttonSecondaryClass}>
                {tr("context.showArchived", "Show archived")}
              </button>
            </div>
          </div>
        ) : null}

        <div className="min-w-0">
          <DndContext sensors={sensors} onDragStart={onDragStart} onDragEnd={onDragEnd} onDragCancel={onDragCancel}>
            <div className={classNames("grid gap-3 md:grid-cols-2", archivedExpanded ? "xl:grid-cols-4" : "xl:grid-cols-3")}>
              <ColumnDropZone columnKey="planned" label={tr("context.planned", "Planned")} items={filteredBoard.planned} tr={tr} ui={ui} syncBusy={syncBusy} selectedTaskId={selectedTaskId} onSelectTask={onSelectTask} onMoveTaskToStatus={onMoveTaskToStatus} />
              <ColumnDropZone columnKey="active" label={tr("context.active", "Active")} items={filteredBoard.active} tr={tr} ui={ui} syncBusy={syncBusy} selectedTaskId={selectedTaskId} onSelectTask={onSelectTask} onMoveTaskToStatus={onMoveTaskToStatus} />
              <ColumnDropZone columnKey="done" label={tr("context.done", "Done")} items={filteredBoard.done} tr={tr} ui={ui} syncBusy={syncBusy} selectedTaskId={selectedTaskId} onSelectTask={onSelectTask} onMoveTaskToStatus={onMoveTaskToStatus} />
              {archivedExpanded ? (
                <ColumnDropZone columnKey="archived" label={tr("context.archived", "Archived")} items={filteredBoard.archived} tr={tr} ui={ui} syncBusy={syncBusy} selectedTaskId={selectedTaskId} onSelectTask={onSelectTask} onMoveTaskToStatus={onMoveTaskToStatus} />
              ) : null}
            </div>
            <DragOverlay>{dragTaskId && taskMap.get(dragTaskId) ? <TaskGhostCard task={taskMap.get(dragTaskId)!} tr={tr} mutedTextClass={ui.mutedTextClass} subtleTextClass={ui.subtleTextClass} /> : null}</DragOverlay>
          </DndContext>
        </div>
      </div>
    </section>
  );
}
