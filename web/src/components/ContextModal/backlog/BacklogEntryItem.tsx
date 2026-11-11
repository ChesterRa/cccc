import { useId } from "react";
import type { BacklogEntry, BacklogTaskLink, Task } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { CheckIcon, ChevronDownIcon, ChevronRightIcon } from "../../Icons";
import { LazyMarkdownRenderer } from "../../LazyMarkdownRenderer";
import { taskStatusLabel, type ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";
import { entryBlocked, firstLine, sectionLabel, type EntryForm } from "./backlogModel";

const OPEN_STATUSES = new Set(["planned", "active"]);

/** A task's status as a mark: a ring when planned, filled when active, a check when done. */
function TaskStatusMark({ status }: { status: string }) {
  if (status === "done") return <CheckIcon size={12} aria-hidden="true" />;
  if (!OPEN_STATUSES.has(status)) return null;
  return (
    <span
      aria-hidden="true"
      className={classNames(
        "inline-block size-2 rounded-full border border-current",
        status === "active" && "bg-current",
      )}
    />
  );
}

function LinkedTask({
  task,
  tr,
  onOpen,
}: {
  task: BacklogTaskLink;
  tr: ContextTranslator;
  onOpen: (taskId: string) => void;
}) {
  const label = tr("context.backlogTaskChip", "{{id}} · {{status}} · {{title}}", {
    id: task.id,
    status: taskStatusLabel(task.status, tr),
    title: task.title || task.id,
  });
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      data-backlog-task={task.status}
      onClick={() => onOpen(task.id)}
      className={classNames(
        "inline-flex min-h-7 items-center gap-1 rounded-md px-1.5 text-xs tabular-nums focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
        "hover:bg-[var(--color-bg-hover)]",
        OPEN_STATUSES.has(task.status)
          ? "text-[var(--color-text-secondary)]"
          : "text-[var(--color-text-muted)]",
      )}
    >
      <TaskStatusMark status={task.status} />
      {task.id}
    </button>
  );
}

function problemText(code: string, tr: ContextTranslator): string {
  switch (code) {
    case "malformed_id":
      return tr("context.backlogProblemMalformedId", "Its ID in the heading is damaged.");
    case "duplicate_id":
      return tr("context.backlogProblemDuplicateId", "Another entry has the same ID.");
    case "empty_title":
      return tr("context.backlogProblemEmptyTitle", "Its title is empty.");
    default:
      return code;
  }
}

export function BacklogEntryItem({
  domId,
  entry,
  closed,
  expanded,
  form,
  moveTargets,
  writable,
  busy,
  returning,
  isDark,
  tr,
  ui,
  onToggle,
  onOpenTask,
  onStartForm,
  onFormChange,
  onSubmit,
  onCancel,
  onMove,
  onReturnWrite,
  onReturnClose,
  onReturnDismiss,
}: {
  domId: string;
  entry: BacklogEntry;
  closed: boolean;
  expanded: boolean;
  /** This entry's open form, if any. */
  form: EntryForm | null;
  moveTargets: string[];
  /** False while the file has a problem that blocks every write, such as an open code block. */
  writable: boolean;
  busy: boolean;
  /** A task whose result is being taken back to this entry. */
  returning: Task | null;
  isDark: boolean;
  tr: ContextTranslator;
  ui: ContextModalUi;
  onToggle: () => void;
  onOpenTask: (taskId: string) => void;
  onStartForm: (kind: "edit" | "close" | "promote" | "move") => void;
  onFormChange: (form: EntryForm) => void;
  onSubmit: () => void;
  onCancel: () => void;
  onMove: (section: string) => void;
  onReturnWrite: () => void;
  onReturnClose: () => void;
  onReturnDismiss: () => void;
}) {
  const bodyId = useId();
  const titleId = useId();
  const textId = useId();
  const blocked = entryBlocked(entry);
  const canWrite = writable && !blocked;
  const openTask = entry.tasks.find((task) => OPEN_STATUSES.has(task.status));
  const returnedOutcome = returning ? firstLine(returning.outcome) : "";

  const cancelButton = (
    <button type="button" className={ui.buttonSecondaryClass} disabled={busy} onClick={onCancel}>
      {tr("context.cancel", "Cancel")}
    </button>
  );

  return (
    <li id={domId} data-backlog-entry="" className="min-w-0 scroll-mt-4">
      <div className="flex min-w-0 items-start gap-1">
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={bodyId}
          onClick={onToggle}
          className={classNames(
            "flex min-h-9 min-w-0 flex-1 items-start gap-1.5 rounded-md px-1 py-1.5 text-left text-sm focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
            "hover:bg-[var(--color-bg-hover)]",
            closed ? "text-[var(--color-text-secondary)]" : "text-[var(--color-text-primary)]",
          )}
        >
          {expanded ? (
            <ChevronDownIcon size={14} className="mt-0.5 shrink-0" aria-hidden="true" />
          ) : (
            <ChevronRightIcon size={14} className="mt-0.5 shrink-0" aria-hidden="true" />
          )}
          <span className="min-w-0 break-words">{entry.title || "—"}</span>
        </button>
        {entry.tasks.length > 0 ? (
          <div className="flex shrink-0 flex-wrap justify-end gap-0.5 pt-0.5">
            {entry.tasks.map((task) => (
              <LinkedTask key={task.id} task={task} tr={tr} onOpen={onOpenTask} />
            ))}
          </div>
        ) : null}
      </div>
      {expanded ? (
        <div id={bodyId} className="space-y-3 pb-3 pl-6 pr-1 pt-1">
          {blocked ? (
            <p className="text-xs text-amber-700 dark:text-amber-400">
              {entry.problems
                .filter((problem) => problem.code !== "missing_id")
                .map((problem) => problemText(problem.code, tr))
                .join(" ")}{" "}
              {tr("context.backlogFixInFile", "Fix it in the file to change this entry here.")}
            </p>
          ) : null}
          {returning ? (
            <div
              data-backlog-return=""
              className="space-y-2 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2 text-sm"
            >
              <p className="break-words">
                {returnedOutcome
                  ? tr("context.backlogReturnBanner", "Result of {{id}}: {{outcome}}", {
                      id: returning.id,
                      outcome: returnedOutcome,
                    })
                  : tr("context.backlogReturnNoOutcome", "{{id}} has no outcome written yet.", {
                      id: returning.id,
                    })}
              </p>
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className={ui.buttonSecondaryClass}
                  disabled={!canWrite || busy || !returnedOutcome}
                  onClick={onReturnWrite}
                >
                  {tr("context.backlogWriteIntoEntry", "Add to the entry")}
                </button>
                {closed ? null : (
                  <button
                    type="button"
                    className={ui.buttonSecondaryClass}
                    disabled={!canWrite || busy}
                    onClick={onReturnClose}
                  >
                    {tr("context.backlogClose", "Close…")}
                  </button>
                )}
                <button type="button" className={ui.buttonSecondaryClass} onClick={onReturnDismiss}>
                  {tr("context.backlogDismiss", "Not now")}
                </button>
              </div>
            </div>
          ) : null}
          {form?.kind === "edit" ? (
            <form
              className="space-y-3"
              onSubmit={(event) => {
                event.preventDefault();
                onSubmit();
              }}
            >
              <div className="space-y-1">
                <label htmlFor={titleId} className="block text-xs font-medium">
                  {tr("context.titleField", "Title")}
                </label>
                <input
                  id={titleId}
                  className={ui.inputClass}
                  value={form.title}
                  readOnly={busy}
                  onChange={(event) => onFormChange({ ...form, title: event.target.value })}
                />
              </div>
              <div className="space-y-1">
                <label htmlFor={textId} className="block text-xs font-medium">
                  {tr("context.backlogBodyLabel", "Text")}
                </label>
                <textarea
                  id={textId}
                  className={classNames(ui.textareaClass, "min-h-[160px] font-mono text-xs")}
                  value={form.body}
                  readOnly={busy}
                  placeholder={tr(
                    "context.backlogBodyPlaceholder",
                    "The problem, why it waits, and when to look at it again. Use #### for headings.",
                  )}
                  onChange={(event) => onFormChange({ ...form, body: event.target.value })}
                />
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  type="submit"
                  className={ui.buttonPrimaryClass}
                  disabled={busy || !form.title.trim()}
                >
                  {tr("context.backlogSave", "Save")}
                </button>
                {cancelButton}
              </div>
            </form>
          ) : (
            <>
              {entry.body.trim() ? (
                <LazyMarkdownRenderer
                  content={entry.body}
                  isDark={isDark}
                  className={classNames("text-sm", ui.subtleTextClass)}
                  fallback={
                    <div
                      className={classNames(
                        "whitespace-pre-wrap break-words text-sm",
                        ui.subtleTextClass,
                      )}
                    >
                      {entry.body}
                    </div>
                  }
                />
              ) : (
                <p className={classNames("text-sm", ui.mutedTextClass)}>
                  {tr("context.backlogNoBody", "No text yet.")}
                </p>
              )}
              {form?.kind === "close" ? (
                <form
                  className="space-y-2"
                  onSubmit={(event) => {
                    event.preventDefault();
                    onSubmit();
                  }}
                >
                  <label htmlFor={textId} className="block text-xs font-medium">
                    {tr("context.backlogOutcomeLabel", "Outcome, in one line")}
                  </label>
                  <input
                    id={textId}
                    className={ui.inputClass}
                    value={form.outcome}
                    readOnly={busy}
                    autoFocus
                    onChange={(event) =>
                      onFormChange({
                        ...form,
                        outcome: event.target.value.replace(/[\r\n]+/g, " "),
                      })
                    }
                  />
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="submit"
                      className={ui.buttonPrimaryClass}
                      disabled={busy || !form.outcome.trim()}
                    >
                      {tr("context.backlogCloseConfirm", "Close entry")}
                    </button>
                    {cancelButton}
                  </div>
                </form>
              ) : form?.kind === "promote" ? (
                <form
                  className="space-y-2"
                  onSubmit={(event) => {
                    event.preventDefault();
                    onSubmit();
                  }}
                >
                  <label htmlFor={textId} className="block text-xs font-medium">
                    {tr("context.backlogPromoteTitle", "Task title")}
                  </label>
                  <input
                    id={textId}
                    className={ui.inputClass}
                    value={form.title}
                    readOnly={busy}
                    autoFocus
                    onChange={(event) => onFormChange({ ...form, title: event.target.value })}
                  />
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="submit"
                      className={ui.buttonPrimaryClass}
                      disabled={busy || !form.title.trim()}
                    >
                      {tr("context.backlogPromoteConfirm", "Create task")}
                    </button>
                    {cancelButton}
                  </div>
                </form>
              ) : form?.kind === "move" ? (
                <div className="space-y-2">
                  <p className="text-xs font-medium">{tr("context.backlogMoveTo", "Move to")}</p>
                  <div className="flex flex-wrap gap-2">
                    {moveTargets.map((name) => (
                      <button
                        key={name || "(preamble)"}
                        type="button"
                        className={ui.buttonSecondaryClass}
                        disabled={busy}
                        onClick={() => onMove(name)}
                      >
                        {sectionLabel(name, tr)}
                      </button>
                    ))}
                    {cancelButton}
                  </div>
                </div>
              ) : canWrite ? (
                <div className="flex flex-wrap gap-2">
                  {closed ? null : openTask ? (
                    <button
                      type="button"
                      className={ui.buttonSecondaryClass}
                      onClick={() => onOpenTask(openTask.id)}
                    >
                      {tr("context.backlogOpenTask", "Open {{id}}", { id: openTask.id })}
                    </button>
                  ) : (
                    <button
                      type="button"
                      className={ui.buttonSecondaryClass}
                      disabled={busy}
                      onClick={() => onStartForm("promote")}
                    >
                      {tr("context.backlogStartTask", "Create task…")}
                    </button>
                  )}
                  <button
                    type="button"
                    className={ui.buttonSecondaryClass}
                    disabled={busy}
                    onClick={() => onStartForm("edit")}
                  >
                    {tr("context.editButton", "Edit")}
                  </button>
                  {moveTargets.length > 0 ? (
                    <button
                      type="button"
                      className={ui.buttonSecondaryClass}
                      disabled={busy}
                      onClick={() => onStartForm("move")}
                    >
                      {closed
                        ? tr("context.backlogReopen", "Reopen…")
                        : tr("context.backlogMove", "Move to…")}
                    </button>
                  ) : null}
                  {closed ? null : (
                    <button
                      type="button"
                      className={ui.buttonSecondaryClass}
                      disabled={busy}
                      onClick={() => onStartForm("close")}
                    >
                      {tr("context.backlogClose", "Close…")}
                    </button>
                  )}
                </div>
              ) : null}
            </>
          )}
        </div>
      ) : null}
    </li>
  );
}
