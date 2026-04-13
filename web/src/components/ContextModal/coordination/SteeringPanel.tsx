import type { ReactNode } from "react";
import type { CoordinationBrief, CoordinationNote, Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import {
  noteTimestamp,
  taskOptionLabel,
  type BriefDraft,
  type ContextTranslator,
  type NoteDraft,
  type SteeringTab,
} from "../model";
import type { ContextModalUi } from "../ui";

interface AttentionCounts {
  blocked: number;
  waitingUser: number;
  pendingHandoffs: number;
}

interface SteeringPanelProps {
  tr: ContextTranslator;
  ui: ContextModalUi;
  brief: CoordinationBrief | null;
  tasksSummary: {
    active?: number;
  };
  attentionCounts: AttentionCounts;
  unassignedCount: number;
  steeringTab: SteeringTab;
  editingBrief: boolean;
  briefDraft: BriefDraft;
  syncBusy: boolean;
  activityBusyKind: "decision" | "handoff" | null;
  activityError: string;
  recentDecisions: CoordinationNote[];
  recentHandoffs: CoordinationNote[];
  decisionDraft: NoteDraft;
  handoffDraft: NoteDraft;
  activeTaskOptions: Task[];
  projectPanel: ReactNode;
  learningPanel: ReactNode;
  onOpenSteeringTab: (tab: SteeringTab) => void;
  onStartBriefEdit: () => void;
  onCancelBriefEdit: () => void;
  onSaveBrief: () => void;
  onBriefDraftChange: (updater: (prev: BriefDraft) => BriefDraft) => void;
  onDecisionDraftChange: (updater: (prev: NoteDraft) => NoteDraft) => void;
  onHandoffDraftChange: (updater: (prev: NoteDraft) => NoteDraft) => void;
  onAddCoordinationNote: (kind: "decision" | "handoff") => void;
}

export function SteeringPanel({
  tr,
  ui,
  brief,
  tasksSummary,
  attentionCounts,
  unassignedCount,
  steeringTab,
  editingBrief,
  briefDraft,
  syncBusy,
  activityBusyKind,
  activityError,
  recentDecisions,
  recentHandoffs,
  decisionDraft,
  handoffDraft,
  activeTaskOptions,
  projectPanel,
  learningPanel,
  onOpenSteeringTab,
  onStartBriefEdit,
  onCancelBriefEdit,
  onSaveBrief,
  onBriefDraftChange,
  onDecisionDraftChange,
  onHandoffDraftChange,
  onAddCoordinationNote,
}: SteeringPanelProps) {
  const tabButtonClass = (active: boolean) => classNames(
    "rounded-lg px-3 py-2 text-sm font-medium transition-colors",
    active
      ? "bg-[var(--glass-accent-bg)] text-[var(--color-accent-primary)]"
      : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
  );
  const notesCardClass = classNames("rounded-xl border p-3 text-sm", "glass-card");

  return (
    <section className={classNames(ui.surfaceClass, "flex min-h-0 flex-1 flex-col p-4")}>
      <div className="flex min-h-0 flex-1 flex-col gap-4">
        <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div className="min-w-0 flex-1">
            <div className={classNames("text-lg font-semibold", "text-[var(--color-text-primary)]")}>{brief?.objective || tr("context.noObjective", "No objective set")}</div>
            <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>{brief?.current_focus || tr("context.noCurrentFocus", "No current focus set")}</div>
            {(brief?.project_brief_stale || (Array.isArray(brief?.constraints) && brief.constraints.length > 0)) ? (
              <div className="mt-3 flex flex-wrap gap-1.5">
                {brief?.project_brief_stale ? <button type="button" onClick={onStartBriefEdit} className={classNames("rounded-full px-2 py-1 text-[11px] transition-colors", "bg-amber-500/15 text-amber-600 dark:text-amber-400 hover:bg-amber-500/25")}>{tr("context.projectBriefNeedsRefresh", "Summary needs refresh")}</button> : null}
                {(brief?.constraints || []).slice(0, 6).map((constraint, index) => <span key={`${constraint}-${index}`} className={classNames("rounded-full px-2 py-1 text-[11px]", "glass-panel text-[var(--color-text-secondary)]")}>{constraint}</span>)}
              </div>
            ) : null}
          </div>
          <div className="flex flex-wrap items-center gap-2 xl:max-w-[18rem] xl:justify-end">
            <span className={classNames("rounded-full px-2.5 py-1 text-xs", "bg-blue-500/15 text-blue-600 dark:text-blue-400")}>{tr("context.active", "Active")} · {Number(tasksSummary.active || 0)}</span>
            <span className={classNames("rounded-full px-2.5 py-1 text-xs", "bg-rose-500/15 text-rose-600 dark:text-rose-400")}>{tr("context.blocked", "Blocked")} · {attentionCounts.blocked}</span>
            <span className={classNames("rounded-full px-2.5 py-1 text-xs", "bg-violet-500/15 text-violet-600 dark:text-violet-400")}>{tr("context.waitingUser", "Waiting user")} · {attentionCounts.waitingUser}</span>
            <span className={classNames("rounded-full px-2.5 py-1 text-xs", "glass-panel text-[var(--color-text-secondary)]")}>{tr("context.unassigned", "Unassigned")} · {unassignedCount}</span>
          </div>
        </div>

        <div className={classNames("flex flex-col gap-3 border-t pt-4", "border-[var(--glass-border-subtle)]")}>
          <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
            <div>
              <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.steering", "Project steering")}</div>
              <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.projectSteeringHint", "Steer the project here. Keep PROJECT.md as the full repository reference, and keep the working summary hot and short.")}</div>
            </div>
          </div>

          <div className={classNames("inline-flex w-fit rounded-2xl border p-1", "glass-panel border-[var(--glass-border-subtle)]")}>
            <button type="button" onClick={() => onOpenSteeringTab("summary")} className={tabButtonClass(steeringTab === "summary")}>{tr("context.brief", "Summary")}</button>
            <button type="button" onClick={() => onOpenSteeringTab("project")} className={tabButtonClass(steeringTab === "project")}>{tr("context.projectMd", "PROJECT.md")}</button>
            <button type="button" onClick={() => onOpenSteeringTab("log")} className={tabButtonClass(steeringTab === "log")}>{tr("context.activityLog", "Activity")}</button>
            <button type="button" onClick={() => onOpenSteeringTab("learning")} className={tabButtonClass(steeringTab === "learning")}>{tr("context.learning", "Learning")}</button>
          </div>

          {steeringTab === "summary" ? (
            <section className={classNames("rounded-xl border p-4", "glass-card")}>
              <div className="flex items-start justify-between gap-3">
                <div>
                  <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.brief", "Summary")}</div>
                  <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{brief?.updated_at ? `${tr("context.updated", "Updated {{time}}", { time: noteTimestamp({ at: brief.updated_at }) })}` : tr("context.notUpdatedYet", "Not updated yet")}</div>
                </div>
                <div className="flex items-center gap-2">
                  {editingBrief ? <span className={classNames("rounded-full px-2 py-0.5 text-[11px] font-medium", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>{tr("context.unsaved", "Unsaved")}</span> : null}
                  {editingBrief ? (
                    <>
                      <button type="button" onClick={onCancelBriefEdit} disabled={syncBusy} className={ui.buttonSecondaryClass}>{tr("context.cancel", "Cancel")}</button>
                      <button type="button" onClick={onSaveBrief} disabled={syncBusy} className={ui.buttonPrimaryClass}>{syncBusy ? tr("context.saving", "Saving…") : tr("context.saveBrief", "Save summary")}</button>
                    </>
                  ) : (
                    <button type="button" onClick={onStartBriefEdit} className={ui.buttonPrimaryClass}>{tr("context.editButton", "Edit")}</button>
                  )}
                </div>
              </div>

              {editingBrief ? (
                <div className="mt-4 grid gap-3 lg:grid-cols-2">
                  <label className="block text-sm lg:col-span-1">
                    <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.objective", "Objective")}</span>
                    <input value={briefDraft.objective} onChange={(event) => onBriefDraftChange((prev) => ({ ...prev, objective: event.target.value }))} className={ui.inputClass} />
                  </label>
                  <label className="block text-sm lg:col-span-1">
                    <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.currentFocus", "Current focus")}</span>
                    <input value={briefDraft.currentFocus} onChange={(event) => onBriefDraftChange((prev) => ({ ...prev, currentFocus: event.target.value }))} className={ui.inputClass} />
                  </label>
                  <label className="block text-sm lg:col-span-2">
                    <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.constraints", "Constraints")}</span>
                    <textarea value={briefDraft.constraints} onChange={(event) => onBriefDraftChange((prev) => ({ ...prev, constraints: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[110px]")} placeholder={tr("context.constraintsPlaceholder", "One per line")} />
                  </label>
                  <label className="block text-sm lg:col-span-2">
                    <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.projectBrief", "Working summary")}</span>
                    <textarea value={briefDraft.projectBrief} onChange={(event) => onBriefDraftChange((prev) => ({ ...prev, projectBrief: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[180px]")} />
                  </label>
                  <label className={classNames("flex items-center gap-2 rounded-lg border px-3 py-2 text-sm lg:col-span-2", "glass-card text-[var(--color-text-primary)]")}>
                    <input type="checkbox" checked={briefDraft.projectBriefStale} onChange={(event) => onBriefDraftChange((prev) => ({ ...prev, projectBriefStale: event.target.checked }))} />
                    {tr("context.projectBriefStale", "Mark working summary as stale")}
                  </label>
                </div>
              ) : (
                <div className="mt-4 space-y-4">
                  <div>
                    <div className={classNames("text-[11px] font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.projectBrief", "Working summary")}</div>
                    <div className={classNames("mt-1 whitespace-pre-wrap text-sm", ui.subtleTextClass)}>{brief?.project_brief || tr("context.noProjectBrief", "No working summary set")}</div>
                  </div>
                  <div>
                    <div className={classNames("text-[11px] font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.constraints", "Constraints")}</div>
                    <div className="mt-2 flex flex-wrap gap-1.5">
                      {Array.isArray(brief?.constraints) && brief.constraints.length > 0 ? brief.constraints.map((constraint, index) => (
                        <span key={`${constraint}-${index}`} className={classNames("rounded-full px-2 py-1 text-[11px]", "glass-panel text-[var(--color-text-secondary)]")}>{constraint}</span>
                      )) : <span className={ui.mutedTextClass}>{tr("context.noConstraints", "No constraints set")}</span>}
                    </div>
                  </div>
                </div>
              )}
            </section>
          ) : null}

          {steeringTab === "project" ? projectPanel : null}

          {steeringTab === "learning" ? learningPanel : null}

          {steeringTab === "log" ? (
            <div className="space-y-3">
              {activityError ? <div className={classNames("rounded-xl border px-3 py-2 text-sm", "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400")}>{activityError}</div> : null}
              <div className="grid gap-3 xl:grid-cols-2">
                <section className={classNames("rounded-xl border p-4", "glass-card")}>
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.recentDecisions", "Recent decisions")}</div>
                      <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.activityWriteHint", "Capture durable decisions and handoffs here when chat alone is too transient.")}</div>
                    </div>
                  </div>
                  <div className={classNames("mt-3 rounded-xl border p-3", "glass-card")}>
                    <textarea value={decisionDraft.summary} onChange={(event) => onDecisionDraftChange((prev) => ({ ...prev, summary: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[96px]")} placeholder={tr("context.decisionPlaceholder", "Record a durable decision or constraint...")} />
                    <div className="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
                      <select value={decisionDraft.taskId} onChange={(event) => onDecisionDraftChange((prev) => ({ ...prev, taskId: event.target.value }))} className={ui.inputClass}>
                        <option value="">{tr("context.noRelatedTask", "No related task")}</option>
                        {activeTaskOptions.map((task) => <option key={task.id} value={task.id}>{taskOptionLabel(task)}</option>)}
                      </select>
                      <button type="button" onClick={() => onAddCoordinationNote("decision")} disabled={activityBusyKind !== null || !String(decisionDraft.summary || "").trim()} className={ui.buttonPrimaryClass}>{activityBusyKind === "decision" ? tr("context.saving", "Saving…") : tr("context.addDecision", "Add decision")}</button>
                    </div>
                  </div>
                  <div className="mt-3 space-y-2">
                    {recentDecisions.length > 0 ? recentDecisions.map((note, index) => (
                      <div key={`${note.summary}-${index}`} className={notesCardClass}>
                        <div className={classNames("font-medium", "text-[var(--color-text-primary)]")}>{note.summary}</div>
                        <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{[note.by || "", note.task_id || "", noteTimestamp(note)].filter(Boolean).join(" · ") || tr("context.noMetadata", "No metadata")}</div>
                      </div>
                    )) : <div className={classNames("text-sm", ui.mutedTextClass)}>{tr("context.noRecentDecisions", "No recent decisions")}</div>}
                  </div>
                </section>
                <section className={classNames("rounded-xl border p-4", "glass-card")}>
                  <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.recentHandoffs", "Recent handoffs")}</div>
                  <div className={classNames("mt-3 rounded-xl border p-3", "glass-card")}>
                    <textarea value={handoffDraft.summary} onChange={(event) => onHandoffDraftChange((prev) => ({ ...prev, summary: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[96px]")} placeholder={tr("context.handoffPlaceholder", "Record a durable handoff or next-owner note...")} />
                    <div className="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
                      <select value={handoffDraft.taskId} onChange={(event) => onHandoffDraftChange((prev) => ({ ...prev, taskId: event.target.value }))} className={ui.inputClass}>
                        <option value="">{tr("context.noRelatedTask", "No related task")}</option>
                        {activeTaskOptions.map((task) => <option key={task.id} value={task.id}>{taskOptionLabel(task)}</option>)}
                      </select>
                      <button type="button" onClick={() => onAddCoordinationNote("handoff")} disabled={activityBusyKind !== null || !String(handoffDraft.summary || "").trim()} className={ui.buttonPrimaryClass}>{activityBusyKind === "handoff" ? tr("context.saving", "Saving…") : tr("context.addHandoff", "Add handoff")}</button>
                    </div>
                  </div>
                  <div className="mt-3 space-y-2">
                    {recentHandoffs.length > 0 ? recentHandoffs.map((note, index) => (
                      <div key={`${note.summary}-${index}`} className={notesCardClass}>
                        <div className={classNames("font-medium", "text-[var(--color-text-primary)]")}>{note.summary}</div>
                        <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{[note.by || "", note.task_id || "", noteTimestamp(note)].filter(Boolean).join(" · ") || tr("context.noMetadata", "No metadata")}</div>
                      </div>
                    )) : <div className={classNames("text-sm", ui.mutedTextClass)}>{tr("context.noRecentHandoffs", "No recent handoffs")}</div>}
                  </div>
                </section>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}
