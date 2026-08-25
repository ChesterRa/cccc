import { useCallback, useEffect, useRef, useState } from "react";
import { fetchTaskById } from "../../services/api";
import type { Task } from "../../types";
import type { TaskDeleteInfoById } from "./taskBoardData";

export function useTaskDetailData({
  groupId,
  isOpen,
  onTaskLoaded,
  onTaskMissing,
  getTasksVersion = () => "",
}: {
  groupId: string;
  isOpen: boolean;
  onTaskLoaded: (task: Task) => void;
  onTaskMissing: (taskId: string) => void;
  getTasksVersion?: () => string;
}) {
  const [detailTasks, setDetailTasks] = useState<Record<string, Task>>({});
  const [deleteInfoById, setDeleteInfoById] = useState<TaskDeleteInfoById>({});
  const groupIdRef = useRef(groupId);
  groupIdRef.current = groupId;
  const getTasksVersionRef = useRef(getTasksVersion);
  getTasksVersionRef.current = getTasksVersion;
  const requests = useRef(new Map<string, Promise<Task | null>>());
  const loadedTasks = useRef<{ groupId: string; tasks: Map<string, Task> }>({
    groupId,
    tasks: new Map(),
  });
  if (loadedTasks.current.groupId !== groupId) {
    loadedTasks.current = { groupId, tasks: new Map() };
  }

  const removeCurrentTask = useCallback(
    (taskId: string) => {
      loadedTasks.current.tasks.delete(taskId);
      setDetailTasks((current) => omit(current, taskId));
      setDeleteInfoById((current) => omit(current, taskId));
      onTaskMissing(taskId);
    },
    [onTaskMissing],
  );

  useEffect(() => {
    if (isOpen) return;
    loadedTasks.current.tasks.clear();
    setDetailTasks({});
    setDeleteInfoById({});
  }, [isOpen]);

  useEffect(() => {
    requests.current.clear();
    setDetailTasks({});
    setDeleteInfoById({});
  }, [groupId]);

  const loadTaskDetail = useCallback(
    (taskId: string): Promise<Task | null> => {
      const id = taskId.trim();
      if (!id || !groupId) return Promise.resolve(null);
      const requestVersion = getTasksVersionRef.current();
      const requestKey = taskRequestKey(groupId, id, requestVersion);
      const pending = requests.current.get(requestKey);
      if (pending) return pending;
      const requestGroupId = groupId;
      const request = fetchTaskById(groupId, id)
        .then((response) => {
          if (requestGroupId !== groupIdRef.current) return null;
          if (requestVersion !== getTasksVersionRef.current()) return null;
          if (!response.ok) {
            if (response.error?.code === "task_not_found") removeCurrentTask(id);
            return null;
          }
          if (requestVersion && response.result.tasksVersion !== requestVersion) return null;
          loadedTasks.current.tasks.set(id, response.result.task);
          onTaskLoaded(response.result.task);
          setDetailTasks((current) => ({ ...current, [id]: response.result.task }));
          setDeleteInfoById((current) => ({ ...current, [id]: response.result.deleteInfo }));
          return response.result.task;
        })
        .finally(() => {
          if (requests.current.get(requestKey) === request) requests.current.delete(requestKey);
        });
      requests.current.set(requestKey, request);
      return request;
    },
    [groupId, onTaskLoaded, removeCurrentTask],
  );

  const refreshTaskDetail = useCallback(
    async (taskId: string) => {
      const id = taskId.trim();
      await requests.current.get(taskRequestKey(groupId, id, getTasksVersionRef.current()));
      return loadTaskDetail(taskId);
    },
    [groupId, loadTaskDetail],
  );

  const forgetTask = useCallback(
    (taskId: string) => removeCurrentTask(taskId),
    [removeCurrentTask],
  );
  const getLoadedTask = useCallback(
    (taskId: string) =>
      loadedTasks.current.groupId === groupId ? loadedTasks.current.tasks.get(taskId) : undefined,
    [groupId],
  );

  return {
    detailTasks,
    deleteInfoById,
    loadTaskDetail,
    refreshTaskDetail,
    forgetTask,
    getLoadedTask,
  };
}

function taskRequestKey(groupId: string, taskId: string, tasksVersion: string): string {
  return `${groupId}\u0000${taskId}\u0000${tasksVersion}`;
}

function omit<T>(record: Record<string, T>, key: string): Record<string, T> {
  const next = { ...record };
  delete next[key];
  return next;
}
