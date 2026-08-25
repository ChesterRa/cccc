import type { Task } from "../../types";
import type {
  TaskDetailResult,
  TaskAttentionFilter,
  TaskPageFacets,
  TaskPageResult,
  TaskPageStatus,
} from "../../services/api";

export const TASK_PAGE_SIZE = 30;
export const TASK_PAGE_STATUSES: readonly TaskPageStatus[] = [
  "planned",
  "active",
  "done",
  "archived",
];
export const PRIMARY_TASK_PAGE_STATUSES: readonly TaskPageStatus[] = ["planned", "active", "done"];

export interface TaskColumnPage {
  items: Task[];
  totalCount: number;
  nextOffset: number;
  hasMore: boolean;
  loading: boolean;
}

export interface UseTaskBoardDataOptions {
  groupId: string;
  isOpen: boolean;
  query: string;
  assignee: string;
  filter: "all" | TaskAttentionFilter;
  includeArchived: boolean;
  contextTasksVersion?: string;
  selectedTaskId?: string;
}

export type TaskColumnPages = Record<TaskPageStatus, TaskColumnPage>;
export type TaskDeleteInfoById = Record<string, TaskDetailResult["deleteInfo"]>;

export const EMPTY_TASK_FACETS: TaskPageFacets = {
  statusCounts: {},
  blocked: 0,
  waitingUser: 0,
  pendingHandoffs: 0,
  unassigned: 0,
  assignees: [],
};

export function taskPageError(response: { error?: { message?: string } }): string {
  return response.error?.message || "Failed to load tasks";
}

export function shouldRefreshTaskRevision(current: string, external: string): boolean {
  if (!current || !external || current === external) return false;
  const currentRevision = /^tasksv:(\d+)$/.exec(current)?.[1];
  const externalRevision = /^tasksv:(\d+)$/.exec(external)?.[1];
  if (currentRevision && externalRevision)
    return Number(externalRevision) > Number(currentRevision);
  return true;
}

function emptyColumn(): TaskColumnPage {
  return { items: [], totalCount: 0, nextOffset: 0, hasMore: false, loading: false };
}

export function createTaskColumnPages(): TaskColumnPages {
  return {
    planned: emptyColumn(),
    active: emptyColumn(),
    done: emptyColumn(),
    archived: emptyColumn(),
  };
}

export function prepareTaskColumnsForReload(
  current: TaskColumnPages,
  statuses: readonly TaskPageStatus[],
  resetAll: boolean,
): TaskColumnPages {
  const next = resetAll
    ? createTaskColumnPages()
    : statuses.includes("archived")
      ? { ...current }
      : { ...current, archived: createTaskColumnPages().archived };
  for (const status of statuses) next[status] = { ...next[status], items: [], loading: true };
  return next;
}

export function applyTaskPage(
  columns: TaskColumnPages,
  status: TaskPageStatus,
  page: TaskPageResult,
  append: boolean,
): TaskColumnPages {
  const existing = append ? columns[status].items : [];
  const byId = new Map(existing.map((task) => [task.id, task]));
  for (const task of page.tasks) byId.set(task.id, task);
  return {
    ...columns,
    [status]: {
      items: Array.from(byId.values()),
      totalCount: page.totalCount,
      nextOffset: append
        ? Math.max(columns[status].nextOffset, page.offset + page.count)
        : page.offset + page.count,
      hasMore: page.hasMore,
      loading: false,
    },
  };
}

export function upsertTaskDetail(columns: TaskColumnPages, task: Task): TaskColumnPages {
  const status = task.status as TaskPageStatus;
  if (!TASK_PAGE_STATUSES.includes(status)) return columns;
  const next = createTaskColumnPages();
  for (const columnStatus of TASK_PAGE_STATUSES) {
    const current = columns[columnStatus];
    const items = current.items.filter((candidate) => candidate.id !== task.id);
    next[columnStatus] = { ...current, items };
  }
  const target = next[status];
  const existingIndex = columns[status].items.findIndex((candidate) => candidate.id === task.id);
  const items = [...target.items];
  if (existingIndex >= 0) {
    items.splice(Math.min(existingIndex, items.length), 0, task);
  } else {
    items.push(task);
  }
  next[status] = { ...target, items, totalCount: Math.max(target.totalCount, items.length) };
  return next;
}

export function removeTaskDetail(columns: TaskColumnPages, taskId: string): TaskColumnPages {
  const next = { ...columns };
  for (const status of TASK_PAGE_STATUSES) {
    const current = columns[status];
    next[status] = { ...current, items: current.items.filter((task) => task.id !== taskId) };
  }
  return next;
}
