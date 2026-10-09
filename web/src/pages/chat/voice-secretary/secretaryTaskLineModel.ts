import type { SecretaryTaskSummary } from "../../../types";
import type { SecretaryTaskCoverage } from "./useSecretaryTasks";

export type SecretaryTaskLineSummary =
  | { kind: "running"; count: number; pending: number }
  | { kind: "queued"; count: number; pending: number }
  | { kind: "attention"; count: number }
  | { kind: "recent"; count: number }
  | { kind: "idle" };

const ATTENTION_PHASES = new Set(["needs_user", "conflict", "failed", "unconfirmed"]);

export function secretaryTaskRunning(task: SecretaryTaskSummary): boolean {
  return ["queued", "starting", "running"].includes(task.phase);
}

export function secretaryTaskNeedsAttention(task: SecretaryTaskSummary): boolean {
  if (task.superseded_by) return false;
  if (task.phase === "cancelled") return false;
  return ATTENTION_PHASES.has(task.phase) || !!task.projection_error;
}

export function summarizeSecretaryTasks(
  tasks: SecretaryTaskSummary[],
  coverage: Pick<SecretaryTaskCoverage, "unprocessed">,
): SecretaryTaskLineSummary {
  const running = tasks.filter((task) => task.phase === "starting" || task.phase === "running");
  const queued = tasks.filter((task) => task.phase === "queued");
  const pending = coverage.unprocessed;
  if (running.length) return { kind: "running", count: running.length + queued.length, pending };
  if (queued.length) return { kind: "queued", count: queued.length, pending };
  const attention = tasks.filter(secretaryTaskNeedsAttention).length;
  if (attention) return { kind: "attention", count: attention };
  if (tasks.length) return { kind: "recent", count: tasks.length };
  return { kind: "idle" };
}
