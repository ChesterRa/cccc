import type { Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { evaluateTaskWorkflow, type TaskAttemptVerdict } from "../../../utils/taskWorkflow";
import { waitingLabel, type ContextTranslator } from "../model";

function latestAttemptTone(verdict: TaskAttemptVerdict): string {
  if (verdict === "keep") return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400";
  if (verdict === "discard") return "bg-amber-500/15 text-amber-600 dark:text-amber-400";
  if (verdict === "crash") return "bg-rose-500/15 text-rose-600 dark:text-rose-400";
  return "glass-panel text-[var(--color-text-secondary)]";
}

function latestAttemptLabel(verdict: TaskAttemptVerdict, tr: ContextTranslator): string {
  if (verdict === "keep") return tr("context.latestAttemptKeep", "Latest keep");
  if (verdict === "discard") return tr("context.latestAttemptDiscard", "Latest discard");
  if (verdict === "crash") return tr("context.latestAttemptCrash", "Latest crash");
  if (verdict === "continue") return tr("context.latestAttemptContinue", "Latest continue");
  return "";
}

export function TaskCardBadges({
  task,
  tr,
  compact = false,
}: {
  task: Task;
  tr: ContextTranslator;
  compact?: boolean;
}) {
  const blocked = Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
  const waiting = String(task.waiting_on || "none").trim();
  const handoff = String(task.handoff_to || "").trim();
  const workflow = evaluateTaskWorkflow({
    parent_id: task.parent_id,
    task_type: task.task_type,
    status: task.status,
    assignee: task.assignee,
    outcome: task.outcome,
    notes: task.notes,
    checklist: task.checklist,
  });
  const missingCurrentBest =
    workflow.isOptimization && !workflow.needsContract && !workflow.hasCurrentBest;

  return (
    <div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">
      {task.assignee ? (
        <span className="max-w-full truncate rounded-full px-2 py-0.5 glass-panel text-[var(--color-text-secondary)]">
          {task.assignee}
        </span>
      ) : !compact ? (
        <span className="rounded-full bg-[var(--glass-tab-bg)] px-2 py-0.5 text-[var(--color-text-muted)]">
          {tr("context.unassigned", "Unassigned")}
        </span>
      ) : null}
      {!compact && task.priority ? (
        <span className="rounded-full px-2 py-0.5 glass-panel text-[var(--color-text-secondary)]">
          {task.priority}
        </span>
      ) : null}
      {blocked ? (
        <span className="rounded-full bg-rose-500/15 px-2 py-0.5 text-rose-600 dark:text-rose-400">
          {tr("context.blocked", "Blocked")}
        </span>
      ) : null}
      {!compact && waiting && waiting !== "none" ? (
        <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-amber-600 dark:text-amber-400">
          {waitingLabel(waiting, tr)}
        </span>
      ) : null}
      {!compact && handoff ? (
        <span
          className={classNames(
            "rounded-full border px-2 py-0.5",
            "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white",
          )}
        >
          {tr("context.handoffTo", "Handoff →")} {handoff}
        </span>
      ) : null}
      {workflow.isOptimization && workflow.latestAttemptVerdict ? (
        <span
          className={classNames(
            "rounded-full px-2 py-0.5",
            latestAttemptTone(workflow.latestAttemptVerdict),
          )}
        >
          {latestAttemptLabel(workflow.latestAttemptVerdict, tr)}
        </span>
      ) : null}
      {missingCurrentBest ? (
        <span className="rounded-full bg-amber-500/12 px-2 py-0.5 text-amber-700 dark:text-amber-300">
          {tr("context.noCurrentBestYet", "No best yet")}
        </span>
      ) : null}
      {workflow.needsContract ? (
        <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-amber-600 dark:text-amber-400">
          {tr("context.needsContract", "Needs requirements")}
        </span>
      ) : null}
      {workflow.needsCloseout ? (
        <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-amber-600 dark:text-amber-400">
          {tr("context.needsCloseout", "Needs closeout")}
        </span>
      ) : null}
    </div>
  );
}
