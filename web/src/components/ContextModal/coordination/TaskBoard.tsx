import type { DragEndEvent, DragStartEvent } from "@dnd-kit/core";
import type { SensorDescriptor, SensorOptions } from "@dnd-kit/core";
import type { Task } from "../../../types";
import type { TaskColumnPages } from "../../../features/contextModal/taskBoardData";
import { classNames } from "../../../utils/classNames";
import type {
  BoardColumns,
  BoardStatus,
  ContextTranslator,
  SubtaskProgress,
  TaskFilterValue,
} from "../model";
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
  matchingCount?: number | null;
  taskMap: Map<string, Task>;
  subtaskProgress: ReadonlyMap<string, SubtaskProgress>;
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
  onRestoreTask: (task: Task) => void;
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
  matchingCount,
  taskMap,
  subtaskProgress,
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
  onRestoreTask,
  onLoadMore,
  onRetryLoad,
}: TaskBoardProps) {
  const hasActiveFilters = Boolean(
    taskQuery.trim() || assigneeFilter !== "__all__" || taskFilter !== "all",
  );
  const showEmpty = !hasVisibleTasks && !taskLoading && !taskLoadError;
  const hasRenderedTasks =
    filteredBoard.planned.length > 0 ||
    filteredBoard.active.length > 0 ||
    filteredBoard.done.length > 0 ||
    (archivedExpanded && filteredBoard.archived.length > 0);
  const showBoard = !showEmpty && (hasRenderedTasks || (!taskLoading && !taskLoadError));
  const emptyBoard =
    showEmpty && !hasActiveFilters && !hasArchivedTasks && Number(tasksSummary.total || 0) === 0;

  return (
    <section className="min-w-0 border-t border-[var(--glass-border-subtle)] pt-3">
      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-3">
          <div className="text-lg font-semibold text-[var(--color-text-primary)]">
            {tr("context.tasks", "Tasks")}
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
          matchingCount={
            hasActiveFilters && !syncBusy && !taskLoading && !taskLoadError ? matchingCount : null
          }
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

        {showEmpty ? (
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
            ) : emptyBoard ? (
              tr("context.noTasksYet", "No tasks yet")
            ) : (
              <>
                {tr("context.noMatchingTasks", "No tasks match the current filters")}
                {hasActiveFilters ? (
                  <button
                    type="button"
                    onClick={onClearFilters}
                    className={classNames(ui.buttonSecondaryClass, "mt-3 block")}
                  >
                    {tr("context.clearFilters", "Clear filters")}
                  </button>
                ) : null}
              </>
            )}
          </div>
        ) : null}

        {showBoard ? (
          <TaskBoardGrid
            tr={tr}
            ui={ui}
            syncBusy={syncBusy}
            archivedExpanded={archivedExpanded}
            board={filteredBoard}
            pages={columnPages}
            taskMap={taskMap}
            subtaskProgress={subtaskProgress}
            selectedTaskId={selectedTaskId}
            dragTaskId={dragTaskId}
            sensors={sensors}
            onDragStart={onDragStart}
            onDragEnd={onDragEnd}
            onDragCancel={onDragCancel}
            onSelectTask={onSelectTask}
            onMoveTaskToStatus={onMoveTaskToStatus}
            onRestoreTask={onRestoreTask}
            onLoadMore={onLoadMore}
          />
        ) : null}
      </div>
    </section>
  );
}
