import { useId, useState } from "react";
import { ChevronDownIcon } from "../../Icons";
import type { CoordinationBrief } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { noteTimestamp, type BriefDraft, type ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";

interface AttentionCounts {
  blocked: number;
  waitingUser: number;
  pendingHandoffs: number;
}

interface WorkingSummaryPanelProps {
  tr: ContextTranslator;
  ui: ContextModalUi;
  brief: CoordinationBrief | null;
  tasksSummary: { active?: number };
  attentionCounts: AttentionCounts;
  unassignedCount: number;
  editingBrief: boolean;
  briefDraft: BriefDraft;
  syncBusy: boolean;
  onStartBriefEdit: () => void;
  onCancelBriefEdit: () => void;
  onSaveBrief: () => void;
  onBriefDraftChange: (updater: (prev: BriefDraft) => BriefDraft) => void;
}

export function WorkingSummaryPanel({
  tr,
  ui,
  brief,
  tasksSummary,
  attentionCounts,
  unassignedCount,
  editingBrief,
  briefDraft,
  syncBusy,
  onStartBriefEdit,
  onCancelBriefEdit,
  onSaveBrief,
  onBriefDraftChange,
}: WorkingSummaryPanelProps) {
  const [summaryExpanded, setSummaryExpanded] = useState(false);
  const summaryId = useId();
  const summaryOpen = summaryExpanded || editingBrief;
  const editBrief = () => {
    setSummaryExpanded(true);
    onStartBriefEdit();
  };
  const hasOverview = Boolean(
    brief?.objective ||
    brief?.current_focus ||
    brief?.project_brief_stale ||
    tasksSummary.active ||
    attentionCounts.blocked ||
    attentionCounts.waitingUser ||
    unassignedCount,
  );
  const hasSummary = Boolean(brief?.project_brief || brief?.constraints?.length);

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <div className="flex min-h-0 flex-1 flex-col gap-4">
        {hasOverview ? (
          <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div className="min-w-0 flex-1">
              <div
                className={classNames("text-lg font-semibold", "text-[var(--color-text-primary)]")}
              >
                {brief?.objective || tr("context.brief", "Summary")}
              </div>
              {brief?.current_focus ? (
                <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>
                  {brief.current_focus}
                </div>
              ) : null}
              {brief?.project_brief_stale ? (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  <button
                    type="button"
                    onClick={editBrief}
                    className={classNames(
                      "rounded-full px-2 py-1 text-xs transition-colors",
                      "bg-amber-500/15 text-amber-600 dark:text-amber-400 hover:bg-amber-500/25",
                    )}
                  >
                    {tr("context.projectBriefNeedsRefresh", "Summary needs refresh")}
                  </button>
                </div>
              ) : null}
            </div>
            <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--color-text-secondary)]">
              {Number(tasksSummary.active || 0) > 0 ? (
                <span>
                  {tr("context.active", "Active")} · {tasksSummary.active}
                </span>
              ) : null}
              {attentionCounts.blocked > 0 ? (
                <span className="rounded-full bg-rose-500/15 px-2 py-1 text-rose-600 dark:text-rose-300">
                  {tr("context.blocked", "Blocked")} · {attentionCounts.blocked}
                </span>
              ) : null}
              {attentionCounts.waitingUser > 0 ? (
                <span className="rounded-full bg-amber-500/15 px-2 py-1 text-amber-700 dark:text-amber-300">
                  {tr("context.waitingUser", "Waiting user")} · {attentionCounts.waitingUser}
                </span>
              ) : null}
              {unassignedCount > 0 ? (
                <span>
                  {tr("context.unassigned", "Unassigned")} · {unassignedCount}
                </span>
              ) : null}
            </div>
          </div>
        ) : null}

        <div
          className={classNames(
            "flex flex-col gap-3",
            hasOverview ? "border-t pt-4" : "",
            "border-[var(--glass-border-subtle)]",
          )}
        >
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <button
              type="button"
              aria-expanded={summaryOpen}
              aria-controls={summaryId}
              onClick={() => {
                if (!editingBrief) setSummaryExpanded((expanded) => !expanded);
              }}
              className="inline-flex min-h-11 items-center gap-1.5 rounded-md text-sm font-medium text-[var(--color-text-primary)] hover:text-[var(--color-text-secondary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
            >
              <ChevronDownIcon
                size={14}
                aria-hidden="true"
                className={summaryOpen ? "rotate-180" : ""}
              />
              {tr("context.projectBrief", "Working summary")}
            </button>
            {!summaryOpen && brief?.updated_at ? (
              <span className={classNames("text-xs", ui.mutedTextClass)}>
                {tr("context.updated", "Updated {{time}}", {
                  time: noteTimestamp({ at: brief.updated_at }),
                })}
              </span>
            ) : null}
            {!summaryOpen ? (
              <button
                type="button"
                onClick={editBrief}
                className={classNames("ml-auto", ui.buttonSecondaryClass)}
              >
                {tr("context.editButton", "Edit")}
              </button>
            ) : null}
          </div>

          <section id={summaryId} hidden={!summaryOpen} className="min-w-0">
            {summaryOpen ? (
              <>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    {!editingBrief && !hasSummary ? (
                      <p className={classNames("text-sm", ui.subtleTextClass)}>
                        {tr(
                          "context.emptySummaryHint",
                          "Add a working summary and constraints to guide the next task.",
                        )}
                      </p>
                    ) : brief?.updated_at ? (
                      <p className={classNames("text-xs", ui.mutedTextClass)}>
                        {tr("context.updated", "Updated {{time}}", {
                          time: noteTimestamp({ at: brief.updated_at }),
                        })}
                      </p>
                    ) : null}
                  </div>
                  <div className="flex items-center gap-2">
                    {editingBrief ? (
                      <span
                        className={classNames(
                          "rounded-full px-2 py-0.5 text-xs font-medium",
                          "bg-amber-500/15 text-amber-600 dark:text-amber-400",
                        )}
                      >
                        {tr("context.unsaved", "Unsaved")}
                      </span>
                    ) : null}
                    {editingBrief ? (
                      <>
                        <button
                          type="button"
                          onClick={onCancelBriefEdit}
                          disabled={syncBusy}
                          className={ui.buttonSecondaryClass}
                        >
                          {tr("context.cancel", "Cancel")}
                        </button>
                        <button
                          type="button"
                          onClick={onSaveBrief}
                          disabled={syncBusy}
                          className={ui.buttonPrimaryClass}
                        >
                          {syncBusy
                            ? tr("context.saving", "Saving…")
                            : tr("context.saveBrief", "Save summary")}
                        </button>
                      </>
                    ) : (
                      <button type="button" onClick={editBrief} className={ui.buttonPrimaryClass}>
                        {tr("context.editButton", "Edit")}
                      </button>
                    )}
                  </div>
                </div>

                {editingBrief ? (
                  <div className="mt-4 grid gap-3 lg:grid-cols-2">
                    <label className="block text-sm lg:col-span-1">
                      <span
                        className={classNames(
                          "mb-1 block text-xs font-medium uppercase tracking-wide",
                          ui.mutedTextClass,
                        )}
                      >
                        {tr("context.objective", "Objective")}
                      </span>
                      <input
                        value={briefDraft.objective}
                        onChange={(event) =>
                          onBriefDraftChange((prev) => ({ ...prev, objective: event.target.value }))
                        }
                        className={ui.inputClass}
                      />
                    </label>
                    <label className="block text-sm lg:col-span-1">
                      <span
                        className={classNames(
                          "mb-1 block text-xs font-medium uppercase tracking-wide",
                          ui.mutedTextClass,
                        )}
                      >
                        {tr("context.currentFocus", "Current focus")}
                      </span>
                      <input
                        value={briefDraft.currentFocus}
                        onChange={(event) =>
                          onBriefDraftChange((prev) => ({
                            ...prev,
                            currentFocus: event.target.value,
                          }))
                        }
                        className={ui.inputClass}
                      />
                    </label>
                    <label className="block text-sm lg:col-span-2">
                      <span
                        className={classNames(
                          "mb-1 block text-xs font-medium uppercase tracking-wide",
                          ui.mutedTextClass,
                        )}
                      >
                        {tr("context.constraints", "Constraints")}
                      </span>
                      <textarea
                        value={briefDraft.constraints}
                        onChange={(event) =>
                          onBriefDraftChange((prev) => ({
                            ...prev,
                            constraints: event.target.value,
                          }))
                        }
                        className={classNames(ui.textareaClass, "min-h-[110px]")}
                        placeholder={tr("context.constraintsPlaceholder", "One per line")}
                      />
                    </label>
                    <label className="block text-sm lg:col-span-2">
                      <span
                        className={classNames(
                          "mb-1 block text-xs font-medium uppercase tracking-wide",
                          ui.mutedTextClass,
                        )}
                      >
                        {tr("context.projectBrief", "Working summary")}
                      </span>
                      <textarea
                        value={briefDraft.projectBrief}
                        onChange={(event) =>
                          onBriefDraftChange((prev) => ({
                            ...prev,
                            projectBrief: event.target.value,
                          }))
                        }
                        className={classNames(ui.textareaClass, "min-h-[180px]")}
                      />
                    </label>
                    <label
                      className={classNames(
                        "flex items-center gap-2 rounded-lg border px-3 py-2 text-sm lg:col-span-2",
                        "glass-card text-[var(--color-text-primary)]",
                      )}
                    >
                      <input
                        type="checkbox"
                        checked={briefDraft.projectBriefStale}
                        onChange={(event) =>
                          onBriefDraftChange((prev) => ({
                            ...prev,
                            projectBriefStale: event.target.checked,
                          }))
                        }
                      />
                      {tr("context.projectBriefStale", "Mark working summary as stale")}
                    </label>
                  </div>
                ) : !hasSummary ? null : (
                  <div className="mt-3 space-y-3">
                    {brief?.project_brief ? (
                      <div>
                        <div
                          className={classNames(
                            "text-xs font-medium uppercase tracking-wide",
                            ui.mutedTextClass,
                          )}
                        >
                          {tr("context.projectBrief", "Working summary")}
                        </div>
                        <div
                          className={classNames(
                            "mt-1 whitespace-pre-wrap text-sm",
                            ui.subtleTextClass,
                          )}
                        >
                          {brief.project_brief}
                        </div>
                      </div>
                    ) : null}
                    {brief?.constraints?.length ? (
                      <div>
                        <div className={classNames("text-xs font-medium", ui.mutedTextClass)}>
                          {tr("context.constraints", "Constraints")}
                        </div>
                        <div className="mt-2 flex flex-wrap gap-1.5">
                          {brief.constraints.map((constraint, index) => (
                            <span
                              key={`${constraint}-${index}`}
                              className={classNames(
                                "rounded-full px-2 py-1 text-xs",
                                "glass-panel text-[var(--color-text-secondary)]",
                              )}
                            >
                              {constraint}
                            </span>
                          ))}
                        </div>
                      </div>
                    ) : null}
                  </div>
                )}
              </>
            ) : null}
          </section>
        </div>
      </div>
    </section>
  );
}
