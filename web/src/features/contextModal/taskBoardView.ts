import type { GroupTasksSummary, Task } from "../../types";
import type { BoardColumns } from "../../components/ContextModal/model";
import type { TaskPageFacets } from "../../services/api";
import type { TaskColumnPages } from "./taskBoardData";

export function deriveTaskBoardView(
  columns: TaskColumnPages,
  detailTasks: Record<string, Task>,
  facets: TaskPageFacets,
): {
  board: BoardColumns;
  tasks: Task[];
  taskMap: Map<string, Task>;
  tasksSummary: GroupTasksSummary;
} {
  const board = {
    planned: columns.planned.items,
    active: columns.active.items,
    done: columns.done.items,
    archived: columns.archived.items,
  };
  const taskMap = new Map(
    Object.values(board)
      .flat()
      .map((task) => [task.id, task]),
  );
  for (const task of Object.values(detailTasks)) taskMap.set(task.id, task);
  const tasks = Array.from(taskMap.values());
  const statusCounts = facets.statusCounts;
  const liveTotal = Object.entries(statusCounts).reduce(
    (total, [status, count]) => total + (status === "archived" ? 0 : count),
    0,
  );
  return {
    board,
    tasks,
    taskMap,
    tasksSummary: {
      total: liveTotal,
      planned: statusCounts.planned || 0,
      active: statusCounts.active || 0,
      done: statusCounts.done || 0,
      archived: statusCounts.archived || 0,
    },
  };
}
