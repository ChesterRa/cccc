import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  fetchTaskPage,
  fetchTaskPages,
  type TaskAttentionFilter,
  type TaskPageFacets,
  type TaskPageStatus,
} from "../../services/api";
import type { Task } from "../../types";
import {
  applyTaskPage,
  createTaskColumnPages,
  EMPTY_TASK_FACETS,
  PRIMARY_TASK_PAGE_STATUSES,
  prepareTaskColumnsForReload,
  TASK_PAGE_SIZE,
  TASK_PAGE_STATUSES,
  shouldRefreshTaskRevision,
  taskPageError,
  type TaskColumnPages,
  type UseTaskBoardDataOptions,
} from "./taskBoardData";
import { deriveTaskBoardView } from "./taskBoardView";
import { useExactTaskRetention } from "./useExactTaskRetention";
import { useTaskDetailData } from "./useTaskDetailData";

function useDebouncedValue(value: string, delay: number): string {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(value), delay);
    return () => window.clearTimeout(timer);
  }, [delay, value]);
  return debounced;
}

export function useTaskBoardData(options: UseTaskBoardDataOptions) {
  const query = useDebouncedValue(options.query.trim(), 250);
  const [columns, setColumns] = useState<TaskColumnPages>(createTaskColumnPages);
  const [facets, setFacets] = useState<TaskPageFacets>(EMPTY_TASK_FACETS);
  const [taskIndex, setTaskIndex] = useState<Task[]>([]);
  const [tasksVersion, setTasksVersion] = useState("");
  const [error, setError] = useState("");
  const tasksVersionRef = useRef("");
  const generationRef = useRef(0);
  const abortRef = useRef<AbortController | null>(null);
  const lastReloadGroupIdRef = useRef("");
  const { onTaskLoaded, onTaskMissing, retainSelectedTask, mergeRetainedTasks } =
    useExactTaskRetention(options.selectedTaskId, setColumns);
  const getTasksVersion = useCallback(() => tasksVersionRef.current, []);
  const {
    detailTasks,
    deleteInfoById,
    loadTaskDetail,
    refreshTaskDetail,
    forgetTask,
    getLoadedTask,
  } = useTaskDetailData({
    groupId: options.groupId,
    isOpen: options.isOpen,
    onTaskLoaded,
    onTaskMissing,
    getTasksVersion,
  });
  const attention = options.filter === "all" ? undefined : options.filter;
  const assignee = options.assignee === "__all__" ? "" : options.assignee;

  const rememberVersion = useCallback((value: string) => {
    tasksVersionRef.current = value;
    setTasksVersion(value);
  }, []);

  useEffect(() => {
    abortRef.current?.abort();
    generationRef.current += 1;
    setColumns(createTaskColumnPages());
    tasksVersionRef.current = "";
    setFacets(EMPTY_TASK_FACETS);
    setTaskIndex([]);
    setTasksVersion("");
    setError("");
  }, [options.groupId]);

  const reload = useCallback(async () => {
    if (!options.isOpen || !options.groupId) return;
    const previousGroupId = lastReloadGroupIdRef.current;
    const groupChanged = Boolean(previousGroupId) && previousGroupId !== options.groupId;
    lastReloadGroupIdRef.current = options.groupId;
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    const generation = ++generationRef.current;
    const statuses = options.includeArchived ? TASK_PAGE_STATUSES : PRIMARY_TASK_PAGE_STATUSES;
    setError("");
    setColumns((current) => {
      const next = prepareTaskColumnsForReload(current, statuses, groupChanged);
      return groupChanged ? next : retainSelectedTask(current, next);
    });
    const response = await fetchTaskPages(options.groupId, {
      statuses,
      limit: TASK_PAGE_SIZE,
      query,
      assignee,
      attention: attention as TaskAttentionFilter | undefined,
      includeIndex: true,
      signal: controller.signal,
    });
    if (controller.signal.aborted || generation !== generationRef.current) return;
    if (!response.ok) {
      setColumns((current) => {
        const next = { ...current };
        for (const status of statuses) next[status] = { ...next[status], loading: false };
        return next;
      });
      setError(taskPageError(response));
      return;
    }
    setColumns((current) => {
      let next = current;
      for (const status of statuses) {
        const page = response.result.pages[status];
        if (page) next = applyTaskPage(next, status, page, false);
      }
      if (groupChanged) return next;
      return mergeRetainedTasks(retainSelectedTask(current, next), getLoadedTask);
    });
    setFacets(response.result.facets);
    setTaskIndex(response.result.taskIndex);
    rememberVersion(response.result.tasksVersion);
  }, [
    assignee,
    attention,
    options.groupId,
    options.includeArchived,
    options.isOpen,
    getLoadedTask,
    mergeRetainedTasks,
    query,
    rememberVersion,
    retainSelectedTask,
  ]);

  useEffect(() => {
    void reload();
    return () => abortRef.current?.abort();
  }, [reload]);

  useEffect(() => {
    if (options.isOpen) return;
    generationRef.current += 1;
    lastReloadGroupIdRef.current = "";
    setColumns(createTaskColumnPages());
    setError("");
  }, [options.isOpen]);

  useEffect(() => {
    const externalVersion = String(options.contextTasksVersion || "").trim();
    if (!options.isOpen || !shouldRefreshTaskRevision(tasksVersion, externalVersion)) return;
    const selectedTaskId = String(options.selectedTaskId || "").trim();
    void reload().then(() =>
      selectedTaskId ? refreshTaskDetail(selectedTaskId).then(() => undefined) : undefined,
    );
  }, [
    options.contextTasksVersion,
    options.isOpen,
    options.selectedTaskId,
    refreshTaskDetail,
    reload,
    tasksVersion,
  ]);

  const loadMore = useCallback(
    async (status: TaskPageStatus) => {
      const current = columns[status];
      if (!options.groupId || current.loading || !current.hasMore) return;
      const generation = generationRef.current;
      setColumns((value) => ({ ...value, [status]: { ...value[status], loading: true } }));
      const response = await fetchTaskPage(options.groupId, {
        status,
        offset: current.nextOffset,
        limit: TASK_PAGE_SIZE,
        query,
        assignee,
        attention: attention as TaskAttentionFilter | undefined,
        signal: abortRef.current?.signal,
      });
      if (generation !== generationRef.current) return;
      if (!response.ok) {
        setColumns((value) => ({ ...value, [status]: { ...value[status], loading: false } }));
        setError(taskPageError(response));
        return;
      }
      if (tasksVersionRef.current && response.result.tasksVersion !== tasksVersionRef.current) {
        await reload();
        return;
      }
      setColumns((value) => applyTaskPage(value, status, response.result, true));
      setFacets(response.result.facets);
      rememberVersion(response.result.tasksVersion);
    },
    [assignee, attention, columns, options.groupId, query, reload, rememberVersion],
  );

  const view = useMemo(
    () => deriveTaskBoardView(columns, detailTasks, facets),
    [columns, detailTasks, facets],
  );
  return {
    columns,
    ...view,
    taskIndex,
    facets,
    error,
    deleteInfoById,
    loading: TASK_PAGE_STATUSES.some((status) => columns[status].loading),
    refresh: reload,
    loadMore,
    loadTaskDetail,
    forgetTask,
  };
}
