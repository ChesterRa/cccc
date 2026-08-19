import { DndContext, DragOverlay, type DragEndEvent, type DragStartEvent } from "@dnd-kit/core";
import type { SensorDescriptor, SensorOptions } from "@dnd-kit/core";
import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import type { BoardColumns, BoardStatus, ContextTranslator, TaskFilterValue } from "../model";
import type { ContextModalUi } from "../ui";
import { TaskBoardColumn } from "./TaskBoardColumn";
import { TaskBoardToolbar } from "./TaskBoardToolbar";
import { TaskGhostCard } from "./TaskGhostCard";

interface TaskBoardProps {
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  taskQuery: string;
  assigneeFilter: string;
  assigneeOptions: string[];
  taskFilter: TaskFilterValue;
  tasksSummary: { total?: number; archived?: number };
  attentionCounts: { blocked: number; waitingUser: number; pendingHandoffs: number };
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
  const renderWindowKey = `${taskQuery}\u0000${assigneeFilter}\u0000${taskFilter}`;
  const columnProps = { tr, ui, syncBusy, selectedTaskId, onSelectTask, onMoveTaskToStatus };

  return (
    <section className={classNames(ui.surfaceClass, "p-4")}>
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
          <div>
            <div className="text-lg font-semibold text-[var(--color-text-primary)]">
              {tr("context.tasks", "Tasks")}
            </div>
            <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>
              {tr(
                "context.taskBoardHint",
                "Plan shared work here. Open a card only when you need blockers, handoffs, notes, or checklist detail.",
              )}
            </div>
          </div>
          <button
            type="button"
            onClick={() => onOpenCreate("planned")}
            className={ui.buttonPrimaryClass}
          >
            {tr("context.newTask", "New task")}
          </button>
        </div>

        <TaskBoardToolbar
          tr={tr}
          ui={ui}
          syncBusy={syncBusy}
          taskQuery={taskQuery}
          assigneeFilter={assigneeFilter}
          assigneeOptions={assigneeOptions}
          taskFilter={taskFilter}
          tasksSummary={tasksSummary}
          attentionCounts={attentionCounts}
          unassignedCount={unassignedCount}
          hasArchivedTasks={hasArchivedTasks}
          archivedExpanded={archivedExpanded}
          onTaskQueryChange={onTaskQueryChange}
          onAssigneeFilterChange={onAssigneeFilterChange}
          onTaskFilterChange={onTaskFilterChange}
          onClearFilters={onClearFilters}
          onArchivedExpandedChange={onArchivedExpandedChange}
        />

        {!hasVisibleTasks ? (
          <div className="rounded-xl border border-dashed px-4 py-5 text-sm glass-card text-[var(--color-text-muted)]">
            {hiddenArchivedMatches > 0 ? (
              <>
                <div>
                  {tr(
                    "context.archivedHiddenMatchesDetail",
                    "{{count}} archived tasks match the current filters. Show archived to review them.",
                    { count: hiddenArchivedMatches },
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => onArchivedExpandedChange(true)}
                  className={classNames(ui.buttonSecondaryClass, "mt-3")}
                >
                  {tr("context.showArchived", "Show archived")}
                </button>
              </>
            ) : (
              tr("context.noMatchingTasks", "No tasks match the current filters")
            )}
          </div>
        ) : null}

        <div className="min-w-0">
          <DndContext
            sensors={sensors}
            onDragStart={onDragStart}
            onDragEnd={onDragEnd}
            onDragCancel={onDragCancel}
          >
            <div
              className={classNames(
                "grid gap-3 md:grid-cols-2",
                archivedExpanded ? "xl:grid-cols-4" : "xl:grid-cols-3",
              )}
            >
              <TaskBoardColumn
                key={`planned:${renderWindowKey}`}
                columnKey="planned"
                label={tr("context.planned", "Planned")}
                items={filteredBoard.planned}
                {...columnProps}
              />
              <TaskBoardColumn
                key={`active:${renderWindowKey}`}
                columnKey="active"
                label={tr("context.active", "Active")}
                items={filteredBoard.active}
                {...columnProps}
              />
              <TaskBoardColumn
                key={`done:${renderWindowKey}`}
                columnKey="done"
                label={tr("context.done", "Done")}
                items={filteredBoard.done}
                {...columnProps}
              />
              {archivedExpanded ? (
                <TaskBoardColumn
                  key={`archived:${renderWindowKey}`}
                  columnKey="archived"
                  label={tr("context.archived", "Archived")}
                  items={filteredBoard.archived}
                  {...columnProps}
                />
              ) : null}
            </div>
            <DragOverlay>
              {dragTaskId && taskMap.get(dragTaskId) ? (
                <TaskGhostCard
                  task={taskMap.get(dragTaskId)!}
                  tr={tr}
                  mutedTextClass={ui.mutedTextClass}
                  subtleTextClass={ui.subtleTextClass}
                />
              ) : null}
            </DragOverlay>
          </DndContext>
        </div>
      </div>
    </section>
  );
}
