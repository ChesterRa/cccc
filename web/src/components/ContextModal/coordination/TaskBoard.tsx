import type { DragEndEvent, DragStartEvent } from "@dnd-kit/core";
import type { SensorDescriptor, SensorOptions } from "@dnd-kit/core";
import type { Task } from "../../../types";
import type { TaskColumnPages } from "../../../features/contextModal/taskBoardData";
import { classNames } from "../../../utils/classNames";
import type { BoardColumns, BoardStatus, ContextTranslator, TaskFilterValue } from "../model";
import type { ContextModalUi } from "../ui";
import { TaskBoardGrid } from "./TaskBoardGrid";
import { TaskBoardLoadNotice } from "./TaskBoardLoadNotice";
import { TaskBoardToolbar } from "./TaskBoardToolbar";

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
  columnPages: TaskColumnPages;
  taskLoading: boolean;
  taskLoadError: string;
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
  onLoadMore: (status: BoardStatus) => void;
  onRetryLoad: () => void;
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
  columnPages,
  taskLoading,
  taskLoadError,
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
  onLoadMore,
  onRetryLoad,
}: TaskBoardProps) {
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

        <TaskBoardLoadNotice
          error={taskLoadError}
          loading={taskLoading}
          tr={tr}
          ui={ui}
          onRetry={onRetryLoad}
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

        <TaskBoardGrid
          tr={tr}
          ui={ui}
          syncBusy={syncBusy}
          archivedExpanded={archivedExpanded}
          board={filteredBoard}
          pages={columnPages}
          taskMap={taskMap}
          selectedTaskId={selectedTaskId}
          dragTaskId={dragTaskId}
          sensors={sensors}
          onDragStart={onDragStart}
          onDragEnd={onDragEnd}
          onDragCancel={onDragCancel}
          onSelectTask={onSelectTask}
          onMoveTaskToStatus={onMoveTaskToStatus}
          onLoadMore={onLoadMore}
        />
      </div>
    </section>
  );
}
