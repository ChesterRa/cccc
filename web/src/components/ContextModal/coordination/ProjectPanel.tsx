import type { ProjectMdInfo } from "../../../types";
import { LazyMarkdownRenderer } from "../../LazyMarkdownRenderer";
import { classNames } from "../../../utils/classNames";
import type { ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";

interface ProjectPanelProps {
  expanded?: boolean;
  isDark: boolean;
  tr: ContextTranslator;
  ui: ContextModalUi;
  projectBusy: boolean;
  projectError: string;
  notifyError: string;
  projectNotice: string;
  projectPathLabel: string;
  editingProject: boolean;
  projectMd: ProjectMdInfo | null;
  projectText: string;
  notifyAgents: boolean;
  onExpand: () => void;
  onCancelEdit: () => void;
  onEditProject: () => void;
  onProjectTextChange: (value: string) => void;
  onNotifyAgentsChange: (checked: boolean) => void;
  onSaveProject: () => void;
}

export function ProjectPanel({
  expanded = false,
  isDark,
  tr,
  ui,
  projectBusy,
  projectError,
  notifyError,
  projectNotice,
  projectPathLabel,
  editingProject,
  projectMd,
  projectText,
  notifyAgents,
  onExpand,
  onCancelEdit,
  onEditProject,
  onProjectTextChange,
  onNotifyAgentsChange,
  onSaveProject,
}: ProjectPanelProps) {
  const shellClass = expanded
    ? "flex h-full min-h-0 flex-col"
    : classNames("rounded-xl border p-4", "glass-card");
  const contentClass = expanded ? "mt-4 min-h-0 flex flex-1 flex-col" : "mt-4";
  const textAreaClass = classNames(
    ui.textareaClass,
    expanded ? "min-h-[520px] flex-1" : "min-h-[320px]",
  );
  const markdownContainerClass = classNames(
    expanded
      ? "min-h-0 flex-1 overflow-y-auto rounded-xl border p-4"
      : "max-h-[36rem] overflow-y-auto rounded-xl border p-3",
    "glass-card",
  );

  return (
    <section className={shellClass}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>
            {tr("context.projectMd", "PROJECT.md")}
          </div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>
            {projectBusy ? tr("common:loading", "Loading…") : projectPathLabel}
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {!expanded ? (
            <button
              type="button"
              onClick={onExpand}
              disabled={projectBusy}
              className={ui.buttonSecondaryClass}
            >
              {tr("context.expand", "Expand")}
            </button>
          ) : null}
          {editingProject ? (
            <button
              type="button"
              onClick={onCancelEdit}
              disabled={projectBusy}
              className={ui.buttonSecondaryClass}
            >
              {tr("context.cancel", "Cancel")}
            </button>
          ) : null}
          <button type="button" onClick={onEditProject} className={ui.buttonPrimaryClass}>
            {editingProject
              ? tr("context.editing", "Editing")
              : projectMd?.found
                ? tr("context.editButton", "Edit")
                : tr("context.createButton", "Create")}
          </button>
        </div>
      </div>
      {projectError ? (
        <div
          className={classNames(
            "mt-3 rounded-lg border px-3 py-2 text-sm",
            "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400",
          )}
        >
          {projectError}
        </div>
      ) : null}
      {notifyError ? (
        <div
          className={classNames(
            "mt-3 rounded-lg border px-3 py-2 text-sm",
            "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400",
          )}
        >
          {notifyError}
        </div>
      ) : null}
      {projectNotice ? (
        <div
          className={classNames(
            "mt-3 rounded-lg border px-3 py-2 text-sm",
            "border-emerald-500/30 bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
          )}
        >
          {projectNotice}
        </div>
      ) : null}
      <div className={contentClass}>
        {editingProject ? (
          <>
            <textarea
              value={projectText}
              onChange={(event) => onProjectTextChange(event.target.value)}
              className={textAreaClass}
              placeholder={tr("context.writePlaceholder", "Write your project constitution here…")}
            />
            <label
              className={classNames(
                "mt-3 flex items-center gap-2 rounded-lg border px-3 py-2 text-sm",
                "glass-card text-[var(--color-text-primary)]",
              )}
            >
              <input
                type="checkbox"
                checked={notifyAgents}
                onChange={(event) => onNotifyAgentsChange(event.target.checked)}
              />
              {tr("context.notifyAgents", "Notify the team in chat (@all) after save")}
            </label>
            <div className="mt-3 flex items-center gap-2">
              <button
                type="button"
                onClick={onSaveProject}
                disabled={projectBusy}
                className={ui.buttonPrimaryClass}
              >
                {projectBusy
                  ? tr("context.saving", "Saving…")
                  : tr("context.saveProject", "Save PROJECT.md")}
              </button>
            </div>
          </>
        ) : projectMd?.found && projectMd.content ? (
          <div className={markdownContainerClass}>
            <LazyMarkdownRenderer
              content={String(projectMd.content)}
              isDark={isDark}
              className={classNames("text-sm", ui.subtleTextClass)}
              fallback={
                <div
                  className={classNames(
                    "whitespace-pre-wrap break-words text-sm",
                    ui.subtleTextClass,
                  )}
                >
                  {String(projectMd.content)}
                </div>
              }
            />
          </div>
        ) : (
          <div
            className={classNames(
              "rounded-xl border border-dashed px-3 py-4 text-sm",
              "border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]",
            )}
          >
            {tr("context.noProjectMd", "No PROJECT.md found")}
          </div>
        )}
      </div>
    </section>
  );
}
