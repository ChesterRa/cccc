import type { Dispatch, SetStateAction } from "react";
import type { Task, TaskWaitingOn } from "../../../types";
import { formatFullTime, formatTime } from "../../../utils/time";
import { classNames } from "../../../utils/classNames";
import { type TaskAttemptVerdict, type TaskTypeDefinition, type TaskWorkflowCoverage, type TaskTypeId } from "../../../utils/taskWorkflow";
import { SelectCombobox } from "../../SelectCombobox";
import {
  alignTaskDraftTaskType,
  getWaitingOnOptions,
  statusTone,
  type ContextTranslator,
  type TaskDeleteInfo,
  type TaskDraft,
} from "../model";
import type { ContextModalUi } from "../ui";

interface TaskEditorPanelProps {
  tr: ContextTranslator;
  ui: ContextModalUi;
  taskEditorMode: "none" | "create" | "edit";
  taskDraft: TaskDraft | null;
  hasTaskUnsaved: boolean;
  syncBusy: boolean;
  selectedTask: Task | null;
  selectedTaskDeleteInfo: TaskDeleteInfo;
  selectedTaskDeleteHint: string;
  taskWorkflowCoverage: TaskWorkflowCoverage;
  taskTypeId: TaskTypeId;
  selectedTaskType: TaskTypeDefinition;
  setTaskDraft: Dispatch<SetStateAction<TaskDraft | null>>;
  onTaskTypeChange: (value: TaskTypeId) => void;
  onResetTask: () => void;
  onClose: () => void;
  onDeleteSelectedTask: () => void;
  onSaveTask: () => void;
}

export function TaskEditorPanel({
  tr,
  ui,
  taskEditorMode,
  taskDraft,
  hasTaskUnsaved,
  syncBusy,
  selectedTask,
  selectedTaskDeleteInfo,
  selectedTaskDeleteHint,
  taskWorkflowCoverage,
  taskTypeId,
  selectedTaskType,
  setTaskDraft,
  onTaskTypeChange,
  onResetTask,
  onClose,
  onDeleteSelectedTask,
  onSaveTask,
}: TaskEditorPanelProps) {
  if (taskEditorMode === "none" || !taskDraft) {
    return null;
  }

  const isCreate = taskEditorMode === "create";
  const attemptToneClass = (verdict: TaskAttemptVerdict) => classNames(
    "rounded-full px-2 py-0.5 text-[11px] font-medium",
    verdict === "keep"
      ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400"
      : verdict === "discard"
        ? "bg-amber-500/15 text-amber-600 dark:text-amber-400"
        : verdict === "crash"
          ? "bg-rose-500/15 text-rose-600 dark:text-rose-400"
          : "glass-panel text-[var(--color-text-secondary)]"
  );
  const workflowToneClass = (ok: boolean) => classNames(
    "rounded-full px-2 py-0.5 text-[11px] font-medium",
    ok
      ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400"
      : "bg-amber-500/15 text-amber-600 dark:text-amber-400"
  );
  const workflowSummary = taskWorkflowCoverage.needsCloseout
    ? tr(
      "context.taskWorkflowNeedsCloseout",
      "Closeout is still missing {{items}}.",
      { items: taskWorkflowCoverage.missingCloseout.join(", ") }
    )
    : taskWorkflowCoverage.needsContract
      ? tr(
        "context.taskWorkflowNeedsContract",
        "This task type is still missing {{items}}.",
        { items: taskWorkflowCoverage.missingSetup.join(", ") }
        )
      : taskWorkflowCoverage.isOptimization
        ? (taskWorkflowCoverage.hasCurrentBest && taskWorkflowCoverage.hasFrontierNext
          ? tr(
            "context.taskWorkflowOptimizationReady",
            "This optimization task has a usable metric setup, a current best, and a next frontier."
          )
          : taskWorkflowCoverage.hasCurrentBest
            ? tr(
              "context.taskWorkflowOptimizationBestReady",
              "This optimization task has a usable metric setup and a current best."
            )
            : tr(
              "context.taskWorkflowOptimizationContractReady",
              "This optimization task has a usable baseline / metric setup."
            ))
        : taskWorkflowCoverage.taskTypeFamily === "standard"
          ? tr("context.taskWorkflowStandardReady", "This standard task has the required goal / evidence structure.")
          : tr("context.taskWorkflowFreeReady", "Keep this task lightweight unless more structure would improve control.");
  const latestAttemptLabel = taskWorkflowCoverage.latestAttemptVerdict === "keep"
    ? tr("context.latestAttemptKeep", "Latest keep")
    : taskWorkflowCoverage.latestAttemptVerdict === "discard"
      ? tr("context.latestAttemptDiscard", "Latest discard")
      : taskWorkflowCoverage.latestAttemptVerdict === "crash"
        ? tr("context.latestAttemptCrash", "Latest crash")
        : taskWorkflowCoverage.latestAttemptVerdict === "continue"
          ? tr("context.latestAttemptContinue", "Latest continue")
          : "";
  const waitingOnOptions = getWaitingOnOptions(tr);
  const taskTypeDescription = selectedTaskType.id === "free"
    ? tr("context.taskTypeDescFree", "Keep it lightweight. Use this when extra structure would add more ceremony than control.")
    : selectedTaskType.id === "standard"
      ? tr("context.taskTypeDescStandard", "Use the normal closed-loop contract: goal, evidence, owner, and closeout.")
      : tr("context.taskTypeDescOptimization", "For metric-sensitive work. Capture baseline, metric, verifier boundary, current best, and next frontier.");
  const requirementItems = selectedTaskType.id === "free"
    ? [tr("context.taskTypeReqFree", "Keep the task lightweight unless more structure improves control.")]
    : selectedTaskType.id === "optimization"
      ? [
          tr("context.goal", "Goal"),
          tr("context.successCriteria", "Success criteria"),
          tr("context.requiredEvidence", "Required evidence"),
          tr("context.owner", "Owner"),
          tr("context.baseline", "Baseline"),
          tr("context.primaryMetric", "Primary metric"),
          tr("context.verifierBoundary", "Verifier boundary"),
          tr("context.currentBest", "Current best"),
          tr("context.frontierNext", "Frontier next"),
          tr("context.attemptDecision", "Attempt decision"),
        ]
      : [
          tr("context.goal", "Goal"),
          tr("context.successCriteria", "Success criteria"),
          tr("context.requiredEvidence", "Required evidence"),
          tr("context.owner", "Owner"),
          tr("context.closeoutVerdict", "Closeout verdict"),
          tr("context.verificationSummary", "Verification summary"),
        ];

  return (
    <section className={classNames(ui.surfaceClass, "min-h-full p-4 pb-24 sm:pb-28")}>
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{isCreate ? tr("context.newTask", "New task") : tr("context.taskDetails", "Task editor")}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{isCreate ? tr("context.newTaskHint", "Create a new shared task. Default root work to Standard and subtasks to Free unless more structure is useful.") : (selectedTask?.id || "")}</div>
        </div>
        <div className="flex items-center gap-2">
          {hasTaskUnsaved ? <span className={classNames("rounded-full px-2 py-0.5 text-[11px] font-medium", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>{tr("context.unsaved", "Unsaved")}</span> : null}
          <button type="button" onClick={onResetTask} disabled={syncBusy} className={ui.buttonSecondaryClass}>{isCreate ? tr("context.clear", "Clear") : tr("context.reset", "Reset")}</button>
          <button type="button" onClick={onClose} className={ui.buttonSecondaryClass}>{tr("context.close", "Close")}</button>
        </div>
      </div>

      <div className="mt-4 space-y-3">
        <div className="flex items-center justify-between gap-2">
          <div className={classNames("text-xs", ui.mutedTextClass)}>
            {isCreate ? tr("context.editorInlineHint", "Editing opens in a side panel so the board stays readable.") : (selectedTask?.updated_at ? `${tr("context.updated", "Updated {{time}}", { time: formatTime(selectedTask.updated_at) })}` : tr("context.notUpdatedYet", "Not updated yet"))}
          </div>
          <span className={classNames("rounded-full border px-2 py-0.5 text-[11px] font-medium", statusTone(taskDraft.status))}>{taskDraft.status}</span>
        </div>

        <div className={classNames("rounded-xl border px-3 py-3", "glass-panel")}>
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div className="min-w-0 flex-1">
              <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.taskWorkflow", "Task type")}</div>
              <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{taskTypeDescription}</div>
              <div className="mt-3">
                <div className={classNames("text-[11px] font-medium uppercase tracking-wide", ui.mutedTextClass)}>
                  {tr("context.taskRequirements", "Requirements")}
                </div>
                <ul className={classNames("mt-1 space-y-1 text-xs", "text-[var(--color-text-secondary)]")}>
                  {requirementItems.map((item) => (
                    <li key={item} className="leading-5">{item}</li>
                  ))}
                </ul>
              </div>
            </div>

            <div className="grid gap-2 sm:w-[15rem]">
              <SelectCombobox
                items={[
                  { value: "free", label: tr("context.taskTypeFree", "Free") },
                  { value: "standard", label: tr("context.taskTypeStandard", "Standard") },
                  { value: "optimization", label: tr("context.taskTypeOptimization", "Optimization") },
                ]}
                value={taskTypeId}
                onChange={(value) => onTaskTypeChange(value as TaskTypeId)}
                ariaLabel={tr("context.taskType", "Task type")}
                className={ui.inputClass}
              />
            </div>
          </div>

          <div className="mt-3 flex flex-wrap gap-1.5">
            <span className={classNames("rounded-full px-2 py-0.5 text-[11px] font-medium", "glass-panel text-[var(--color-text-secondary)]")}>
              {selectedTaskType.label}
            </span>
            <span className={classNames("rounded-full px-2 py-0.5 text-[11px] font-medium", "glass-panel text-[var(--color-text-secondary)]")}>
              {taskWorkflowCoverage.isRoot
                ? tr("context.rootTask", "Root task")
                : tr("context.subtask", "Subtask")}
            </span>
            {taskWorkflowCoverage.taskTypeFamily === "standard" || taskWorkflowCoverage.isOptimization ? (
              <>
                <span className={workflowToneClass(taskWorkflowCoverage.hasGoal)}>{tr("context.goal", "Goal")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasSuccessCriteria)}>{tr("context.successCriteria", "Success criteria")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasRequiredEvidence)}>{tr("context.requiredEvidence", "Required evidence")}</span>
              </>
            ) : null}
            {taskWorkflowCoverage.taskTypeFamily === "standard" ? (
              <span className={workflowToneClass(taskWorkflowCoverage.hasOwner)}>{tr("context.owner", "Owner")}</span>
            ) : null}
            {taskWorkflowCoverage.isOptimization ? (
              <>
                <span className={workflowToneClass(taskWorkflowCoverage.hasBaseline)}>{tr("context.baseline", "Baseline")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasPrimaryMetric)}>{tr("context.primaryMetric", "Primary metric")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasVerifierBoundary)}>{tr("context.verifierBoundary", "Verifier boundary")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasCurrentBest)}>{tr("context.currentBest", "Current best")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasFrontierNext)}>{tr("context.frontierNext", "Frontier next")}</span>
              </>
            ) : null}
            {String(taskDraft.status || "").toLowerCase() === "done" ? (
              <>
                <span className={workflowToneClass(taskWorkflowCoverage.hasOutcomeSummary)}>{tr("context.outcomeSummary", "Outcome summary")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasCloseoutVerdict)}>{tr("context.closeoutVerdict", "Closeout verdict")}</span>
                <span className={workflowToneClass(taskWorkflowCoverage.hasVerificationSummary)}>{tr("context.verificationSummary", "Verification summary")}</span>
                {taskWorkflowCoverage.isOptimization ? (
                  <span className={workflowToneClass(taskWorkflowCoverage.hasAttemptDecision)}>{tr("context.attemptDecision", "Attempt decision")}</span>
                ) : null}
              </>
            ) : null}
          </div>

          <div className={classNames(
            "mt-3 rounded-lg border px-3 py-2 text-xs",
            taskWorkflowCoverage.needsContract || taskWorkflowCoverage.needsCloseout
              ? "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
              : "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
          )}>
            {workflowSummary}
          </div>
          {taskWorkflowCoverage.isOptimization && (taskWorkflowCoverage.hasCurrentBest || taskWorkflowCoverage.hasFrontierNext || taskWorkflowCoverage.hasAttemptLog) ? (
            <div className={classNames("mt-3 grid gap-2 rounded-lg border px-3 py-3 text-xs", "glass-card")}>
              {taskWorkflowCoverage.hasCurrentBest ? (
                <div>
                  <div className={classNames("font-medium", ui.mutedTextClass)}>{tr("context.currentBest", "Current best")}</div>
                  <div className={classNames("mt-1", "text-[var(--color-text-primary)]")}>{taskWorkflowCoverage.currentBestSummary}</div>
                </div>
              ) : null}
              {taskWorkflowCoverage.hasFrontierNext ? (
                <div>
                  <div className={classNames("font-medium", ui.mutedTextClass)}>{tr("context.frontierNext", "Frontier next")}</div>
                  <div className={classNames("mt-1", "text-[var(--color-text-primary)]")}>{taskWorkflowCoverage.frontierNextSummary}</div>
                </div>
              ) : null}
              {taskWorkflowCoverage.hasAttemptLog ? (
                <div className={classNames("flex flex-wrap items-center gap-2", (taskWorkflowCoverage.hasCurrentBest || taskWorkflowCoverage.hasFrontierNext) && "border-t pt-2")}>
                  {latestAttemptLabel ? <span className={attemptToneClass(taskWorkflowCoverage.latestAttemptVerdict)}>{latestAttemptLabel}</span> : null}
                  <span className={classNames("min-w-0 flex-1", ui.subtleTextClass)}>
                    {taskWorkflowCoverage.latestAttemptSummary || tr("context.attemptLogPresent", "Attempt log recorded")}
                  </span>
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        <label className="block text-sm">
          <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.titleField", "Title")}</span>
          <input value={taskDraft.title} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, title: event.target.value } : prev)} className={ui.inputClass} />
        </label>

        <label className="block text-sm">
          <div className="flex items-center justify-between gap-2">
            <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.outcome", "Outcome")}</span>
            <span className={classNames("mb-1 block text-[11px]", ui.mutedTextClass)}>{tr("context.outcomeHint", "Use this as the concise result summary, especially at closeout.")}</span>
          </div>
          <textarea value={taskDraft.outcome} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, outcome: event.target.value } : prev)} className={ui.textareaClass} />
        </label>

        <div className="grid gap-3 sm:grid-cols-2">
          <label className="block text-sm">
            <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.status", "Status")}</span>
            <SelectCombobox
              items={[
                { value: "planned", label: tr("context.planned", "Planned") },
                { value: "active", label: tr("context.active", "Active") },
                { value: "done", label: tr("context.done", "Done") },
                { value: "archived", label: tr("context.archived", "Archived") },
              ]}
              value={taskDraft.status}
              onChange={(value) => setTaskDraft((prev) => prev ? { ...prev, status: value } : prev)}
              ariaLabel={tr("context.status", "Status")}
              className={ui.inputClass}
            />
          </label>
          <label className="block text-sm">
            <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.assignee", "Assignee")}</span>
            <input value={taskDraft.assignee} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, assignee: event.target.value } : prev)} className={ui.inputClass} />
          </label>
        </div>

        <label className="block text-sm">
          <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.priority", "Priority")}</span>
          <input value={taskDraft.priority} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, priority: event.target.value } : prev)} className={ui.inputClass} />
        </label>

        <div className="grid gap-3">
          <label className="block text-sm">
            <div className="flex items-center justify-between gap-2">
              <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.notes", "Notes")}</span>
              <span className={classNames("mb-1 block text-[11px]", ui.mutedTextClass)}>{tr("context.notesHint", "Use notes for the working record. The built-in requirements stay read-only above.")}</span>
            </div>
            <textarea value={taskDraft.notes} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, notes: event.target.value } : prev)} className={classNames(ui.textareaClass, "min-h-[220px]")} />
          </label>

          <label className="block text-sm">
            <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.checklist", "Checklist")}</span>
            <textarea value={taskDraft.checklist} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, checklist: event.target.value } : prev)} className={classNames(ui.textareaClass, "min-h-[140px]")} placeholder={tr("context.checklistPlaceholder", "Use [ ], [~], [x] prefixes if useful.")} />
          </label>
        </div>

        <details className={classNames("rounded-xl border px-3 py-3", "glass-card") }>
          <summary className={classNames("cursor-pointer text-sm font-medium", "text-[var(--color-text-primary)]")}>{tr("context.advancedTaskFields", "Advanced details")}</summary>
          <div className="mt-3 space-y-3">
            <div className="grid gap-3 sm:grid-cols-2">
              <label className="block text-sm">
                <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.parentTask", "Parent task")}</span>
                <input
                  value={taskDraft.parentId}
                  onChange={(event) => setTaskDraft((prev) => prev ? {
                    ...prev,
                    parentId: event.target.value,
                    taskType: alignTaskDraftTaskType(prev.taskType, event.target.value, prev.parentId),
                  } : prev)}
                  className={ui.inputClass}
                  placeholder={tr("context.rootTask", "Root task")}
                />
              </label>
              <label className="block text-sm">
                <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.handoffTo", "Handoff to")}</span>
                <input value={taskDraft.handoffTo} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, handoffTo: event.target.value } : prev)} className={ui.inputClass} />
              </label>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <label className="block text-sm">
                <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.blockedBy", "Blocked by")}</span>
                <textarea value={taskDraft.blockedBy} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, blockedBy: event.target.value } : prev)} className={classNames(ui.textareaClass, "min-h-[90px]")} placeholder={tr("context.onePerLine", "One per line")} />
              </label>
              <label className="block text-sm">
                <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.waitingOn", "Waiting on")}</span>
                <SelectCombobox
                  items={waitingOnOptions}
                  value={taskDraft.waitingOn}
                  onChange={(value) => setTaskDraft((prev) => prev ? { ...prev, waitingOn: value as TaskWaitingOn } : prev)}
                  ariaLabel={tr("context.waitingOn", "Waiting on")}
                  className={ui.inputClass}
                />
              </label>
            </div>
          </div>
        </details>

        {isCreate ? (
          <div className={classNames("rounded-xl border px-3 py-3 text-xs", "glass-card text-[var(--color-text-muted)]")}>
            {tr("context.newTaskHint", "Create a new shared task. Default root work to Standard and subtasks to Free unless more structure is useful.")}
          </div>
        ) : selectedTask ? (
          <div className={classNames("grid gap-2 rounded-xl border px-3 py-3 text-xs sm:grid-cols-2", "glass-card text-[var(--color-text-muted)]")}>
            <div>{tr("context.createdAt", "Created")}: {selectedTask.created_at ? formatFullTime(selectedTask.created_at) : "-"}</div>
            <div>{tr("context.updatedAt", "Updated")}: {selectedTask.updated_at ? formatFullTime(selectedTask.updated_at) : "-"}</div>
            <div>{tr("context.archivedFrom", "Archived from")}: {selectedTask.archived_from || "-"}</div>
            <div>{tr("context.checklistItems", "Checklist items")}: {Array.isArray(selectedTask.checklist) ? selectedTask.checklist.length : 0}</div>
          </div>
        ) : null}

        <div className="flex items-center gap-2">
          {!isCreate && selectedTask ? (
            <button
              type="button"
              onClick={onDeleteSelectedTask}
              disabled={syncBusy || !selectedTaskDeleteInfo.allowed}
              className={ui.buttonDangerClass}
              title={selectedTaskDeleteHint || undefined}
            >
              {tr("context.deleteTask", "Delete")}
            </button>
          ) : null}
          <button type="button" onClick={onSaveTask} disabled={syncBusy} className={ui.buttonPrimaryClass}>{syncBusy ? tr("context.saving", "Saving…") : (isCreate ? tr("context.createTask", "Create task") : tr("context.saveTask", "Save task"))}</button>
        </div>
        {!isCreate && selectedTaskDeleteHint ? (
          <div className={classNames("text-xs", ui.mutedTextClass)}>{selectedTaskDeleteHint}</div>
        ) : null}
      </div>
    </section>
  );
}
