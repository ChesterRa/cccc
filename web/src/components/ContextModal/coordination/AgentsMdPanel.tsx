import type { AgentsMdInfo } from "../../../types";
import { LazyMarkdownRenderer } from "../../LazyMarkdownRenderer";
import { classNames } from "../../../utils/classNames";
import type { ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";

interface AgentsMdPanelProps {
  expanded?: boolean;
  isDark: boolean;
  tr: ContextTranslator;
  ui: ContextModalUi;
  agentsMdBusy: boolean;
  agentsMdError: string;
  notifyError: string;
  agentsMdNotice: string;
  agentsMdPathLabel: string;
  editingAgentsMd: boolean;
  agentsMd: AgentsMdInfo | null;
  agentsMdText: string;
  notifyAgents: boolean;
  onExpand: () => void;
  onCancelEdit: () => void;
  onEditAgentsMd: () => void;
  onAgentsMdTextChange: (value: string) => void;
  onNotifyAgentsChange: (checked: boolean) => void;
  onSaveAgentsMd: () => void;
}

export function AgentsMdPanel({
  expanded = false,
  isDark,
  tr,
  ui,
  agentsMdBusy,
  agentsMdError,
  notifyError,
  agentsMdNotice,
  agentsMdPathLabel,
  editingAgentsMd,
  agentsMd,
  agentsMdText,
  notifyAgents,
  onExpand,
  onCancelEdit,
  onEditAgentsMd,
  onAgentsMdTextChange,
  onNotifyAgentsChange,
  onSaveAgentsMd,
}: AgentsMdPanelProps) {
  const shellClass = expanded ? "flex h-full min-h-0 flex-col" : "min-w-0";
  const contentClass = expanded ? "mt-4 min-h-0 flex flex-1 flex-col" : "mt-4";
  const textAreaClass = classNames(
    ui.textareaClass,
    expanded ? "min-h-[520px] flex-1" : "min-h-[320px]",
  );
  const markdownContainerClass = classNames(
    expanded ? "min-h-0 flex-1 overflow-y-auto rounded-xl border p-4" : "rounded-xl border p-3",
    "glass-card",
  );

  return (
    <section className={shellClass}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>
            {tr("context.agentsMd", "AGENTS.md")}
          </div>
          <div className={classNames("mt-1 break-all text-xs", ui.mutedTextClass)}>
            {agentsMdBusy ? tr("common:loading", "Loading…") : agentsMdPathLabel}
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {!expanded ? (
            <button
              type="button"
              onClick={onExpand}
              disabled={agentsMdBusy}
              className={ui.buttonSecondaryClass}
            >
              {tr("context.expand", "Expand")}
            </button>
          ) : null}
          {editingAgentsMd ? (
            <button
              type="button"
              onClick={onCancelEdit}
              disabled={agentsMdBusy}
              className={ui.buttonSecondaryClass}
            >
              {tr("context.cancel", "Cancel")}
            </button>
          ) : null}
          <button type="button" onClick={onEditAgentsMd} className={ui.buttonPrimaryClass}>
            {editingAgentsMd
              ? tr("context.editing", "Editing")
              : agentsMd?.exists
                ? tr("context.editButton", "Edit")
                : tr("context.createButton", "Create")}
          </button>
        </div>
      </div>
      <p className={classNames("mt-3 text-xs", ui.mutedTextClass)}>
        {tr(
          "context.agentsMdHint",
          "Repository instructions in the current workspace. Each runtime controls loading and precedence; saving does not reload running sessions.",
        )}
      </p>
      {agentsMdError ? (
        <div
          className={classNames(
            "mt-3 rounded-lg border px-3 py-2 text-sm",
            "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400",
          )}
        >
          {agentsMdError}
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
      {agentsMdNotice ? (
        <div
          className={classNames(
            "mt-3 rounded-lg border px-3 py-2 text-sm",
            "border-emerald-500/30 bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
          )}
        >
          {agentsMdNotice}
        </div>
      ) : null}
      <div className={contentClass}>
        {editingAgentsMd ? (
          <>
            <textarea
              value={agentsMdText}
              onChange={(event) => onAgentsMdTextChange(event.target.value)}
              className={textAreaClass}
              aria-label={tr("context.agentsMd", "AGENTS.md")}
              placeholder={tr(
                "context.agentsMdPlaceholder",
                "Write repository instructions, conventions, and validation commands…",
              )}
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
                onClick={onSaveAgentsMd}
                disabled={agentsMdBusy}
                className={ui.buttonPrimaryClass}
              >
                {agentsMdBusy
                  ? tr("context.saving", "Saving…")
                  : tr("context.saveAgentsMd", "Save AGENTS.md")}
              </button>
            </div>
          </>
        ) : agentsMd?.exists && agentsMd.content ? (
          <div className={markdownContainerClass}>
            <LazyMarkdownRenderer
              content={String(agentsMd.content)}
              isDark={isDark}
              className={classNames("text-sm", ui.subtleTextClass)}
              fallback={
                <div
                  className={classNames(
                    "whitespace-pre-wrap break-words text-sm",
                    ui.subtleTextClass,
                  )}
                >
                  {String(agentsMd.content)}
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
            {agentsMd?.exists
              ? tr("context.emptyAgentsMd", "AGENTS.md is empty.")
              : tr("context.noAgentsMd", "No AGENTS.md in this workspace.")}
          </div>
        )}
      </div>
    </section>
  );
}
