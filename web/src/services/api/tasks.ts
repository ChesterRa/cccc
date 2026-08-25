import type { Task } from "../../types";
import { apiJson, type ApiResponse } from "./base";
import { normalizeTaskTree, normalizeTasks } from "./taskNormalization";

export type TaskPageStatus = "planned" | "active" | "done" | "archived";
export type TaskAttentionFilter = "blocked" | "waiting_user" | "handoff" | "unassigned";

export interface TaskPageFacets {
  statusCounts: Record<string, number>;
  blocked: number;
  waitingUser: number;
  pendingHandoffs: number;
  unassigned: number;
  assignees: string[];
}

export interface TaskPageResult {
  tasks: Task[];
  count: number;
  totalCount: number;
  offset: number;
  limit: number;
  hasMore: boolean;
  tasksVersion: string;
  facets: TaskPageFacets;
}

export interface TaskDetailResult {
  task: Task;
  tasksVersion: string;
  deleteInfo: { allowed: boolean; total: number; reason: string };
}

export interface TaskPagesResult {
  pages: Partial<Record<TaskPageStatus, TaskPageResult>>;
  tasksVersion: string;
  facets: TaskPageFacets;
  taskIndex: Task[];
}

export interface TaskBatchResult {
  tasks: Task[];
  tasksVersion: string;
}

export interface FetchTaskPageOptions {
  status: TaskPageStatus;
  offset?: number;
  limit?: number;
  query?: string;
  assignee?: string;
  attention?: TaskAttentionFilter;
  signal?: AbortSignal;
}

export interface FetchTaskPagesOptions extends Omit<FetchTaskPageOptions, "status" | "offset"> {
  statuses: readonly TaskPageStatus[];
  includeIndex?: boolean;
}

function numberRecord(value: unknown): Record<string, number> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return Object.fromEntries(
    Object.entries(value).map(([key, count]) => [key, Math.max(0, Number(count) || 0)]),
  );
}

function normalizeFacets(value: unknown): TaskPageFacets {
  const record = value && typeof value === "object" && !Array.isArray(value) ? value : {};
  const item = record as Record<string, unknown>;
  return {
    statusCounts: numberRecord(item.status_counts),
    blocked: Math.max(0, Number(item.blocked) || 0),
    waitingUser: Math.max(0, Number(item.waiting_user) || 0),
    pendingHandoffs: Math.max(0, Number(item.pending_handoffs) || 0),
    unassigned: Math.max(0, Number(item.unassigned) || 0),
    assignees: Array.isArray(item.assignees)
      ? item.assignees.map((entry) => String(entry || "").trim()).filter(Boolean)
      : [],
  };
}

function normalizePage(
  value: unknown,
  tasksVersion: string,
  facets: TaskPageFacets,
  fallbackLimit: number,
): TaskPageResult {
  const raw = value && typeof value === "object" && !Array.isArray(value) ? value : {};
  const item = raw as Record<string, unknown>;
  const tasks = normalizeTasks(item.tasks);
  return {
    tasks,
    count: Math.max(0, Number(item.count) || tasks.length),
    totalCount: Math.max(0, Number(item.total_count) || 0),
    offset: Math.max(0, Number(item.offset) || 0),
    limit: Math.max(1, Number(item.limit) || fallbackLimit),
    hasMore: Boolean(item.has_more),
    tasksVersion,
    facets,
  };
}

function filterParams(options: Omit<FetchTaskPageOptions, "status">): URLSearchParams {
  const params = new URLSearchParams({
    limit: String(Math.min(100, Math.max(1, Math.trunc(options.limit || 30)))),
  });
  if (String(options.query || "").trim()) params.set("query", String(options.query).trim());
  if (String(options.assignee || "").trim())
    params.set("assignee", String(options.assignee).trim());
  if (options.attention) params.set("attention", options.attention);
  return params;
}

export async function fetchTaskPage(
  groupId: string,
  options: FetchTaskPageOptions,
): Promise<ApiResponse<TaskPageResult>> {
  const params = filterParams(options);
  params.set("status", options.status);
  params.set("offset", String(Math.max(0, Math.trunc(options.offset || 0))));
  const response = await apiJson<Record<string, unknown>>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/tasks?${params.toString()}`,
    { signal: options.signal },
  );
  if (!response.ok) return response as ApiResponse<TaskPageResult>;
  const tasksVersion = String(response.result.tasks_version || "");
  const facets = normalizeFacets(response.result.facets);
  return {
    ok: true,
    result: normalizePage(response.result, tasksVersion, facets, options.limit || 30),
  };
}

export async function fetchTaskPages(
  groupId: string,
  options: FetchTaskPagesOptions,
): Promise<ApiResponse<TaskPagesResult>> {
  const params = filterParams(options);
  params.set("statuses", options.statuses.join(","));
  if (options.includeIndex) params.set("include_index", "true");
  const response = await apiJson<Record<string, unknown>>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/tasks?${params.toString()}`,
    { signal: options.signal },
  );
  if (!response.ok) return response as ApiResponse<TaskPagesResult>;
  const tasksVersion = String(response.result.tasks_version || "");
  const facets = normalizeFacets(response.result.facets);
  const rawPages =
    response.result.pages && typeof response.result.pages === "object"
      ? (response.result.pages as Record<string, unknown>)
      : {};
  const pages: TaskPagesResult["pages"] = {};
  for (const status of options.statuses) {
    pages[status] = normalizePage(rawPages[status], tasksVersion, facets, options.limit || 30);
  }
  return {
    ok: true,
    result: { pages, tasksVersion, facets, taskIndex: normalizeTasks(response.result.task_index) },
  };
}

export async function fetchTasksByIds(
  groupId: string,
  taskIds: readonly string[],
  signal?: AbortSignal,
): Promise<ApiResponse<TaskBatchResult>> {
  const params = new URLSearchParams({ task_ids: taskIds.join(",") });
  const response = await apiJson<Record<string, unknown>>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/tasks?${params.toString()}`,
    { signal },
  );
  if (!response.ok) return response as ApiResponse<TaskBatchResult>;
  return {
    ok: true,
    result: {
      tasks: normalizeTasks(response.result.tasks),
      tasksVersion: String(response.result.tasks_version || ""),
    },
  };
}

export async function fetchTaskById(
  groupId: string,
  taskId: string,
  signal?: AbortSignal,
): Promise<ApiResponse<TaskDetailResult>> {
  const params = new URLSearchParams({ task_id: taskId });
  const response = await apiJson<Record<string, unknown>>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/tasks?${params.toString()}`,
    { signal },
  );
  if (!response.ok) return response as ApiResponse<TaskDetailResult>;
  const task = normalizeTaskTree(response.result.task);
  if (!task) {
    return { ok: false, error: { code: "invalid_task", message: "Invalid task response" } };
  }
  const rawDeleteInfo = response.result.delete_info;
  const deleteInfo =
    rawDeleteInfo && typeof rawDeleteInfo === "object" && !Array.isArray(rawDeleteInfo)
      ? (rawDeleteInfo as Record<string, unknown>)
      : {};
  return {
    ok: true,
    result: {
      task,
      tasksVersion: String(response.result.tasks_version || ""),
      deleteInfo: {
        allowed: Boolean(deleteInfo.allowed),
        total: Math.max(0, Number(deleteInfo.total) || 0),
        reason: String(deleteInfo.reason || ""),
      },
    },
  };
}
