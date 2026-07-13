import type { Task, TaskMessageRef } from "../types";

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function trimString(value: unknown): string {
  return typeof value === "string" ? value.trim() : value == null ? "" : String(value).trim();
}

export function isTaskMessageRef(value: unknown): value is TaskMessageRef {
  const record = asRecord(value);
  if (!record) return false;
  return trimString(record.kind) === "task_ref" && !!trimString(record.task_id);
}

export function getTaskMessageRefs(value: unknown): TaskMessageRef[] {
  if (!Array.isArray(value)) return [];
  return value.filter(isTaskMessageRef);
}

function taskStatusValue(task: Task | null | undefined, ref: TaskMessageRef): string {
  return trimString(task?.status || ref.status).toLowerCase();
}

function taskWaitingValue(task: Task | null | undefined, ref: TaskMessageRef): string {
  return trimString(task?.waiting_on || ref.waiting_on).toLowerCase();
}

function taskHandoffValue(task: Task | null | undefined, ref: TaskMessageRef): string {
  return trimString(task?.handoff_to || ref.handoff_to);
}

export type TaskRefStateKey =
  | "planned"
  | "active"
  | "handoff"
  | "waiting_user"
  | "blocked"
  | "done"
  | "archived"
  | "linked";

export function getTaskRefStateKey(ref: TaskMessageRef, task?: Task | null): TaskRefStateKey {
  const status = taskStatusValue(task, ref);
  const waitingOn = taskWaitingValue(task, ref);
  const handoffTo = taskHandoffValue(task, ref);
  const blockedBy = Array.isArray(task?.blocked_by)
    ? task?.blocked_by.filter((item) => trimString(item))
    : [];

  if (status === "done") return "done";
  if (status === "archived") return "archived";
  if (handoffTo) return "handoff";
  if (waitingOn === "user") return "waiting_user";
  if (blockedBy.length > 0 || waitingOn === "external") return "blocked";
  if (status === "active") return "active";
  if (status === "planned") return "planned";
  return "linked";
}

export function getTaskRefChipLabel(ref: TaskMessageRef, task?: Task | null): string {
  const taskId = trimString(task?.id || ref.task_id);
  const title = trimString(task?.title || ref.title);
  if (taskId && title) return `${taskId} · ${title}`;
  return taskId || title || "Task";
}
