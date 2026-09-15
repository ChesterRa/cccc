import { Fragment, memo } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, File, Folder, Loader2 } from "lucide-react";
import { classNames } from "../../utils/classNames";
import type { WorkspaceEntry, WorkspaceGitStatus } from "../../types";
import type { TreeNode } from "./workspaceTreeModel";

type Props = {
  rows: TreeNode[];
  selectedPath: string;
  isDark: boolean;
  onToggleDirectory: (path: string) => void;
  onOpenFile: (path: string) => void;
  onRetryDirectory: (path: string) => void;
  onContextMenu: (entry: WorkspaceEntry, x: number, y: number) => void;
};

const GIT_BADGE: Record<WorkspaceGitStatus, { letter: string; light: string; dark: string }> = {
  modified: { letter: "M", light: "text-amber-600", dark: "text-amber-300" },
  added: { letter: "A", light: "text-emerald-600", dark: "text-emerald-300" },
  deleted: { letter: "D", light: "text-rose-600", dark: "text-rose-300" },
  renamed: { letter: "R", light: "text-violet-600", dark: "text-violet-300" },
  untracked: { letter: "U", light: "text-sky-600", dark: "text-sky-300" },
  conflicted: { letter: "!", light: "text-rose-700", dark: "text-rose-200" },
};

function GitBadge({ status, isDark }: { status: WorkspaceGitStatus; isDark: boolean }) {
  const badge = GIT_BADGE[status];
  return (
    <span
      aria-label={status}
      className={classNames(
        "ml-1 w-3 shrink-0 text-center text-[11px] font-semibold tabular-nums",
        isDark ? badge.dark : badge.light,
      )}
    >
      {badge.letter}
    </span>
  );
}

function WorkspaceTreeRows({
  rows,
  selectedPath,
  isDark,
  onToggleDirectory,
  onOpenFile,
  onRetryDirectory,
  onContextMenu,
}: Props) {
  const { t } = useTranslation("chat");
  return (
    <div role="tree" className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden py-1">
      {rows.map((node) => {
        const { entry } = node;
        const selected = !entry.is_dir && entry.path === selectedPath;
        return (
          <Fragment key={node.key}>
            <button
              type="button"
              role="treeitem"
              aria-expanded={entry.is_dir ? node.expanded : undefined}
              aria-selected={selected}
              title={entry.path}
              style={{ paddingLeft: 8 + node.depth * 12 }}
              onClick={() =>
                entry.is_dir ? onToggleDirectory(entry.path) : onOpenFile(entry.path)
              }
              onContextMenu={(event) => {
                event.preventDefault();
                onContextMenu(entry, event.clientX, event.clientY);
              }}
              className={classNames(
                // Roomier rows on phones, where these are touch targets rather than mouse targets.
                "flex w-full items-center gap-1 py-2 pr-2 text-left text-[13px] transition-colors sm:py-[3px]",
                entry.ignored && "opacity-45",
                selected
                  ? isDark
                    ? "bg-cyan-400/12 text-cyan-100"
                    : "bg-cyan-500/10 text-cyan-900"
                  : isDark
                    ? "text-slate-200 hover:bg-white/6"
                    : "text-slate-700 hover:bg-black/5",
              )}
            >
              <span className="flex w-4 shrink-0 items-center justify-center opacity-70">
                {entry.is_dir ? (
                  node.loading ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : node.expanded ? (
                    <ChevronDown className="h-3.5 w-3.5" />
                  ) : (
                    <ChevronRight className="h-3.5 w-3.5" />
                  )
                ) : null}
              </span>
              <span className="flex w-4 shrink-0 items-center justify-center opacity-60">
                {entry.is_dir ? (
                  <Folder className="h-3.5 w-3.5" />
                ) : (
                  <File className="h-3.5 w-3.5" />
                )}
              </span>
              <span className="truncate">{entry.name}</span>
              {entry.git_status ? <GitBadge status={entry.git_status} isDark={isDark} /> : null}
              {!entry.git_status && entry.git_dirty_descendant ? (
                <span
                  aria-hidden="true"
                  className={classNames(
                    "ml-1 h-1.5 w-1.5 shrink-0 rounded-full",
                    isDark ? "bg-amber-300/70" : "bg-amber-500/70",
                  )}
                />
              ) : null}
            </button>
            {node.error ? (
              <div
                role="alert"
                className="px-3 py-2 text-xs text-[var(--color-text-secondary)]"
                style={{ paddingLeft: 28 + node.depth * 12 }}
              >
                <p className="break-words">{node.error}</p>
                <button
                  type="button"
                  className="mt-1 underline underline-offset-2 focus-visible:outline-2"
                  onClick={() => onRetryDirectory(entry.path)}
                  aria-label={t("workspaceRetryDirectory", {
                    path: entry.path,
                    defaultValue: "Retry loading {{path}}",
                  })}
                >
                  {t("workspaceRetry", { defaultValue: "Retry" })}
                </button>
              </div>
            ) : null}
          </Fragment>
        );
      })}
    </div>
  );
}

/** Rows only change when the listing, selection or theme changes, not on every chat render. */
export const WorkspaceTree = memo(WorkspaceTreeRows);
