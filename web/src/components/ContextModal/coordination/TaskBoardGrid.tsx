import { useMemo } from "react";
import { DndContext, DragOverlay, type DragEndEvent, type DragStartEvent } from "@dnd-kit/core";
import type {
  Announcements,
  ScreenReaderInstructions,
  SensorDescriptor,
  SensorOptions,
  UniqueIdentifier,
} from "@dnd-kit/core";
import type { Task } from "../../../types";
import type { TaskColumnPages } from "../../../features/contextModal/taskBoardData";
import { classNames } from "../../../utils/classNames";
import {
  taskStatusLabel,
  taskTitle,
  type BoardColumns,
  type BoardStatus,
  type ContextTranslator,
  type SubtaskProgress,
} from "../model";
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
  subtaskProgress,
  selectedTaskId,
  dragTaskId,
  sensors,
  onDragStart,
  onDragEnd,
  onDragCancel,
  onSelectTask,
  onMoveTaskToStatus,
  onRestoreTask,
  onLoadMore,
}: {
  tr: ContextTranslator;
  ui: ContextModalUi;
  syncBusy: boolean;
  archivedExpanded: boolean;
  board: BoardColumns;
  pages: TaskColumnPages;
  taskMap: Map<string, Task>;
  subtaskProgress: ReadonlyMap<string, SubtaskProgress>;
  selectedTaskId: string;
  dragTaskId: string;
  sensors: SensorDescriptor<SensorOptions>[];
  onDragStart: (event: DragStartEvent) => void;
  onDragEnd: (event: DragEndEvent) => void;
  onDragCancel: () => void;
  onSelectTask: (task: Task) => void;
  onMoveTaskToStatus: (task: Task, nextStatus: BoardStatus) => void;
  onRestoreTask: (task: Task) => void;
  onLoadMore: (status: BoardStatus) => void;
}) {
  const shared = {
    tr,
    ui,
    syncBusy,
    subtaskProgress,
    selectedTaskId,
    onSelectTask,
    onMoveTaskToStatus,
    onRestoreTask,
    onLoadMore,
  };
  // What a screen reader hears while a task is moved: the task's title and the
  // column's name, in the reader's language, rather than the board's ids.
  const accessibility = useMemo(() => {
    const named = (id: UniqueIdentifier) => {
      const taskId = String(id).replace(/^task:/, "");
      const task = taskMap.get(taskId);
      return { task: (task && taskTitle(task)) || taskId };
    };
    const columnName = (id: UniqueIdentifier) => {
      const key = String(id);
      return key.startsWith("column:") ? taskStatusLabel(key.slice(7), tr) : "";
    };
    const announcements: Announcements = {
      onDragStart: ({ active }) =>
        tr("context.dragStarted", "Picked up {{task}}.", named(active.id)),
      onDragOver: ({ active, over }) =>
        over && columnName(over.id)
          ? tr("context.dragOver", "{{task}} is over {{column}}.", {
              ...named(active.id),
              column: columnName(over.id),
            })
          : tr("context.dragNotOver", "{{task}} is not over a column.", named(active.id)),
      onDragEnd: ({ active, over }) =>
        over && columnName(over.id)
          ? tr("context.dragEnded", "{{task}} was dropped on {{column}}.", {
              ...named(active.id),
              column: columnName(over.id),
            })
          : tr(
              "context.dragEndedNowhere",
              "{{task}} was dropped outside the columns and stays where it was.",
              named(active.id),
            ),
      onDragCancel: ({ active }) =>
        tr(
          "context.dragCancelled",
          "Moving {{task}} was cancelled. It stays where it was.",
          named(active.id),
        ),
    };
    const screenReaderInstructions: ScreenReaderInstructions = {
      draggable: tr(
        "context.dragInstructions",
        "To move a task to another column, press Space or Enter, move it with the arrow keys, then press Space or Enter again to drop it. Press Escape to cancel.",
      ),
    };
    return { announcements, screenReaderInstructions };
  }, [taskMap, tr]);
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
        accessibility={accessibility}
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
          {column("planned", taskStatusLabel("planned", tr))}
          {column("active", taskStatusLabel("active", tr))}
          {column("done", taskStatusLabel("done", tr))}
          {archivedExpanded ? column("archived", taskStatusLabel("archived", tr)) : null}
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
