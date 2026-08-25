import { DndContext, DragOverlay, type DragEndEvent, type DragStartEvent } from "@dnd-kit/core";
import type { SensorDescriptor, SensorOptions } from "@dnd-kit/core";
import type { Task } from "../../../types";
import type { TaskColumnPages } from "../../../features/contextModal/taskBoardData";
import { classNames } from "../../../utils/classNames";
import type { BoardColumns, BoardStatus, ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";
import { TaskBoardColumn } from "./TaskBoardColumn";
import { TaskGhostCard } from "./TaskGhostCard";

export function TaskBoardGrid({
  tr,
  ui,
  syncBusy,
  archivedExpanded,
  board,
  pages,
  taskMap,
  selectedTaskId,
  dragTaskId,
  sensors,
  onDragStart,
  onDragEnd,
  onDragCancel,
  onSelectTask,
  onMoveTaskToStatus,
  onLoadMore,
}: {
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  archivedExpanded: boolean;
  board: BoardColumns;
  pages: TaskColumnPages;
  taskMap: Map<string, Task>;
  selectedTaskId: string;
  dragTaskId: string;
  sensors: SensorDescriptor<SensorOptions>[];
  onDragStart: (event: DragStartEvent) => void;
  onDragEnd: (event: DragEndEvent) => void;
  onDragCancel: () => void;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
  onLoadMore: (status: BoardStatus) => void;
}) {
  const shared = { tr, ui, syncBusy, selectedTaskId, onSelectTask, onMoveTaskToStatus, onLoadMore };
  const column = (status: BoardStatus, label: string) => (
    <TaskBoardColumn
      columnKey={status}
      label={label}
      items={board[status]}
      totalCount={pages[status].totalCount}
      hasMore={pages[status].hasMore}
      loading={pages[status].loading}
      {...shared}
    />
  );
  return (
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
          {column("planned", tr("context.planned", "Planned"))}
          {column("active", tr("context.active", "Active"))}
          {column("done", tr("context.done", "Done"))}
          {archivedExpanded ? column("archived", tr("context.archived", "Archived")) : null}
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
  );
}
