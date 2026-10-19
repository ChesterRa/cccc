import type { CoordinationNote, Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { SelectCombobox } from "../../SelectCombobox";
import { noteTimestamp, taskOptionLabel, type ContextTranslator, type NoteDraft } from "../model";
import type { ContextModalUi } from "../ui";

interface CoordinationLogPanelProps {
  tr: ContextTranslator;
  ui: ContextModalUi;
  activityBusyKind: "decision" | "handoff" | null;
  activityError: string;
  recentDecisions: CoordinationNote[];
  recentHandoffs: CoordinationNote[];
  decisionDraft: NoteDraft;
  handoffDraft: NoteDraft;
  activeTaskOptions: Task[];
  onDecisionDraftChange: (updater: (prev: NoteDraft) => NoteDraft) => void;
  onHandoffDraftChange: (updater: (prev: NoteDraft) => NoteDraft) => void;
  onAddCoordinationNote: (kind: "decision" | "handoff") => void;
}

export function CoordinationLogPanel({
  tr,
  ui,
  activityBusyKind,
  activityError,
  recentDecisions,
  recentHandoffs,
  decisionDraft,
  handoffDraft,
  activeTaskOptions,
  onDecisionDraftChange,
  onHandoffDraftChange,
  onAddCoordinationNote,
}: CoordinationLogPanelProps) {
  const notesCardClass = classNames("rounded-xl border p-3 text-sm", "glass-card");
  return (
    <div className="space-y-3">
      {activityError ? (
        <div
          className={classNames(
            "rounded-xl border px-3 py-2 text-sm",
            "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400",
          )}
        >
          {activityError}
        </div>
      ) : null}
      <div className="grid gap-6 xl:grid-cols-2">
        <section className="min-w-0">
          <div className="flex items-start justify-between gap-3">
            <div>
              <div
                className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}
              >
                {tr("context.recentDecisions", "Recent decisions")}
              </div>
              <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>
                {tr(
                  "context.activityWriteHint",
                  "Capture durable decisions and handoffs here when chat alone is too transient.",
                )}
              </div>
            </div>
          </div>
          <div className="mt-3">
            <textarea
              value={decisionDraft.summary}
              onChange={(event) =>
                onDecisionDraftChange((prev) => ({ ...prev, summary: event.target.value }))
              }
              className={classNames(ui.textareaClass, "min-h-[96px]")}
              placeholder={tr(
                "context.decisionPlaceholder",
                "Record a durable decision or constraint...",
              )}
            />
            <div className="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
              <SelectCombobox
                items={[
                  { value: "", label: tr("context.noRelatedTask", "No related task") },
                  ...activeTaskOptions.map((task) => ({
                    value: task.id,
                    label: taskOptionLabel(task),
                  })),
                ]}
                value={decisionDraft.taskId}
                onChange={(value) => onDecisionDraftChange((prev) => ({ ...prev, taskId: value }))}
                ariaLabel={tr("context.noRelatedTask", "No related task")}
                className={ui.inputClass}
                searchable
              />
              <button
                type="button"
                onClick={() => onAddCoordinationNote("decision")}
                disabled={activityBusyKind !== null || !String(decisionDraft.summary || "").trim()}
                className={ui.buttonPrimaryClass}
              >
                {activityBusyKind === "decision"
                  ? tr("context.saving", "Saving…")
                  : tr("context.addDecision", "Add decision")}
              </button>
            </div>
          </div>
          <div className="mt-3 space-y-2">
            {recentDecisions.length > 0 ? (
              recentDecisions.map((note, index) => (
                <div key={`${note.summary}-${index}`} className={notesCardClass}>
                  <div className={classNames("font-medium", "text-[var(--color-text-primary)]")}>
                    {note.summary}
                  </div>
                  <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>
                    {[note.by || "", note.task_id || "", noteTimestamp(note)]
                      .filter(Boolean)
                      .join(" · ") || tr("context.noMetadata", "No metadata")}
                  </div>
                </div>
              ))
            ) : (
              <div className={classNames("text-sm", ui.mutedTextClass)}>
                {tr("context.noRecentDecisions", "No recent decisions")}
              </div>
            )}
          </div>
        </section>
        <section className="min-w-0">
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>
            {tr("context.recentHandoffs", "Recent handoffs")}
          </div>
          <div className="mt-3">
            <textarea
              value={handoffDraft.summary}
              onChange={(event) =>
                onHandoffDraftChange((prev) => ({ ...prev, summary: event.target.value }))
              }
              className={classNames(ui.textareaClass, "min-h-[96px]")}
              placeholder={tr(
                "context.handoffPlaceholder",
                "Record a durable handoff or next-owner note...",
              )}
            />
            <div className="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
              <SelectCombobox
                items={[
                  { value: "", label: tr("context.noRelatedTask", "No related task") },
                  ...activeTaskOptions.map((task) => ({
                    value: task.id,
                    label: taskOptionLabel(task),
                  })),
                ]}
                value={handoffDraft.taskId}
                onChange={(value) => onHandoffDraftChange((prev) => ({ ...prev, taskId: value }))}
                ariaLabel={tr("context.noRelatedTask", "No related task")}
                className={ui.inputClass}
                searchable
              />
              <button
                type="button"
                onClick={() => onAddCoordinationNote("handoff")}
                disabled={activityBusyKind !== null || !String(handoffDraft.summary || "").trim()}
                className={ui.buttonPrimaryClass}
              >
                {activityBusyKind === "handoff"
                  ? tr("context.saving", "Saving…")
                  : tr("context.addHandoff", "Add handoff")}
              </button>
            </div>
          </div>
          <div className="mt-3 space-y-2">
            {recentHandoffs.length > 0 ? (
              recentHandoffs.map((note, index) => (
                <div key={`${note.summary}-${index}`} className={notesCardClass}>
                  <div className={classNames("font-medium", "text-[var(--color-text-primary)]")}>
                    {note.summary}
                  </div>
                  <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>
                    {[note.by || "", note.task_id || "", noteTimestamp(note)]
                      .filter(Boolean)
                      .join(" · ") || tr("context.noMetadata", "No metadata")}
                  </div>
                </div>
              ))
            ) : (
              <div className={classNames("text-sm", ui.mutedTextClass)}>
                {tr("context.noRecentHandoffs", "No recent handoffs")}
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
