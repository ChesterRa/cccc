import type { ReactNode } from "react";
import type { Task } from "../../../types";
import { evaluateTaskWorkflow, type TaskAttemptVerdict } from "../../../utils/taskWorkflow";
import {
  checklistItemOpen,
  priorityLabel,
  taskIsLive,
  taskStatus,
  waitingLabel,
  type ContextTranslator,
  type SubtaskProgress,
} from "../model";

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
  subtasks,
}: {
  task: Task;
  tr: ContextTranslator;
  compact?: boolean;
  subtasks?: SubtaskProgress;
}) {
  const live = taskIsLive(task);
  // The daemon derives `open_blocked_by` beside the stored list; a projection
  // that predates it falls back to the stored ids.
  const blockedBy = Array.isArray(task.open_blocked_by) ? task.open_blocked_by : task.blocked_by;
  const blocked = live && Array.isArray(blockedBy) && blockedBy.length > 0;
  const waiting = String(task.waiting_on || "none").trim();
  const handoff = String(task.handoff_to || "").trim();
  const checklist = (task.checklist || []).filter((item) => String(item.text || "").trim());
  const checklistDone = checklist.filter((item) => item.status === "done").length;
  // Unknown custom statuses are not proof that an item is still outstanding.
  const checklistRemaining = checklist.filter(checklistItemOpen).length;
  const doneWithRemaining = taskStatus(task) === "done" && checklistRemaining > 0;
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

  // Brackets mark a note that is not a control. Waiting on the user is a
  // filled tag instead, the one state that asks for the reader. Attention
  // comes right after the assignee, ahead of the detail, so a scan reads it first.
  const note = (kind: string, text: ReactNode, title?: string) => (
    <span key={kind} data-task-note={kind} title={title}>
      [{text}]
    </span>
  );
  return (
    <>
      {task.assignee ? (
        <span data-task-note="assignee">{task.assignee}</span>
      ) : !compact ? (
        note("unassigned", tr("context.unassigned", "Unassigned"))
      ) : null}
      {blocked
        ? note("blocked", `${tr("context.blocked", "Blocked")}: ${blockedBy?.join(", ")}`)
        : null}
      {!compact && live && waiting === "user" ? (
        <span data-task-note="waiting-user">{waitingLabel(waiting, tr)}</span>
      ) : !compact && live && waiting && waiting !== "none" ? (
        note("waiting", waitingLabel(waiting, tr))
      ) : null}
      {!compact && live && handoff
        ? note("handoff", `→${handoff}`, `${tr("context.handoffTo", "Handoff →")} ${handoff}`)
        : null}
      {!compact && task.priority ? note("priority", priorityLabel(task.priority, tr)) : null}
      {!compact && task.parent_id
        ? note("parent", `${tr("context.parentTask", "Parent task")} ${task.parent_id}`)
        : null}
      {!compact && checklist.length > 0
        ? note(
            doneWithRemaining ? "checklist-remaining" : "checklist",
            `${tr("context.checklist", "Checklist")} ${checklistDone}/${checklist.length}${
              doneWithRemaining
                ? ` · ${tr("context.checklistRemaining", "{{count}} remaining", { count: checklistRemaining })}`
                : ""
            }`,
          )
        : null}
      {subtasks && subtasks.total > 0
        ? note(
            "subtasks",
            tr("context.subtaskProgress", "Subtasks {{done}}/{{total}}", subtasks),
            tr(
              "context.subtasksHint",
              "Counts direct subtasks; archived ones are excluded. Subtask status does not complete this task.",
            ),
          )
        : null}
      {workflow.isOptimization && workflow.latestAttemptVerdict
        ? note("attempt", latestAttemptLabel(workflow.latestAttemptVerdict, tr))
        : null}
      {missingCurrentBest ? note("best", tr("context.noCurrentBestYet", "No best yet")) : null}
    </>
  );
}
