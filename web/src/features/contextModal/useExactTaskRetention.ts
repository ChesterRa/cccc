import { useCallback, useRef, type Dispatch, type SetStateAction } from "react";
import type { Task } from "../../types";
import { removeTaskDetail, upsertTaskDetail, type TaskColumnPages } from "./taskBoardData";

export function retainTaskDuringReload(
  current: TaskColumnPages,
  next: TaskColumnPages,
  taskId: string,
): TaskColumnPages {
  const id = taskId.trim();
  if (!id) return next;
  const refreshedTaskExists = Object.values(next).some((column) =>
    column.items.some((candidate) => candidate.id === id),
  );
  if (refreshedTaskExists) return next;
  const task = Object.values(current)
    .flatMap((column) => column.items)
    .find((candidate) => candidate.id === id);
  return task ? upsertTaskDetail(next, task) : next;
}

export function useExactTaskRetention(
  selectedTaskId: string | undefined,
  setColumns: Dispatch<SetStateAction<TaskColumnPages>>,
) {
  const selectedTaskIdRef = useRef("");
  selectedTaskIdRef.current = String(selectedTaskId || "").trim();

  const onTaskLoaded = useCallback(
    (task: Task) => {
      setColumns((current) => upsertTaskDetail(current, task));
    },
    [setColumns],
  );
  const onTaskMissing = useCallback(
    (taskId: string) => {
      setColumns((current) => removeTaskDetail(current, taskId));
    },
    [setColumns],
  );
  const retainSelectedTask = useCallback(
    (current: TaskColumnPages, next: TaskColumnPages) =>
      retainTaskDuringReload(current, next, selectedTaskIdRef.current),
    [],
  );
  const mergeRetainedTasks = useCallback(
    (columns: TaskColumnPages, getLoadedTask: (taskId: string) => Task | undefined) => {
      const selectedTask = getLoadedTask(selectedTaskIdRef.current);
      return selectedTask ? upsertTaskDetail(columns, selectedTask) : columns;
    },
    [],
  );

  return { onTaskLoaded, onTaskMissing, retainSelectedTask, mergeRetainedTasks };
}
