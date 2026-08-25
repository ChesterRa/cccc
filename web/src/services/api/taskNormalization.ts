import type { Task } from "../../types";
import { normalizeTask } from "./base";

export function normalizeTasks(value: unknown): Task[] {
  return (Array.isArray(value) ? value : [])
    .map((entry) => normalizeTask(entry))
    .filter((entry): entry is Task => Boolean(entry));
}

export function normalizeTaskTree(value: unknown): Task | null {
  const task = normalizeTask(value);
  if (!task) return null;
  const record = value as Record<string, unknown>;
  if (!Array.isArray(record.children)) return task;
  return {
    ...task,
    children: record.children
      .map((child) => normalizeTaskTree(child))
      .filter((child): child is Task => Boolean(child)),
  };
}
