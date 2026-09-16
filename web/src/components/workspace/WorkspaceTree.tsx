import { Fragment, memo, useEffect, useRef, useState, type DragEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  File,
  FileSymlink,
  Folder,
  FolderSymlink,
  Loader2,
  MoreHorizontal,
} from "lucide-react";
import { classNames } from "../../utils/classNames";
import type { WorkspaceEntry, WorkspaceGitStatus } from "../../types";
import type { TreeNode } from "./workspaceTreeModel";

type Props = {
  onDragEntry?: (event: DragEvent, entry: WorkspaceEntry) => void;
  dropTarget?: string | null;
  rows: TreeNode[];
  selectedPath: string;
  isDark: boolean;
  onToggleDirectory: (path: string) => void;
  onOpenFile: (path: string) => void;
  onRetryDirectory: (path: string) => void;
  onContextMenu: (entry: WorkspaceEntry, x: number, y: number, anchor: HTMLElement) => void;
  revealRequest?: { path: string } | null;
  onReveal: (row: HTMLElement) => void;
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
  onDragEntry,
  dropTarget,
  rows,
  selectedPath,
  isDark,
  onToggleDirectory,
  onOpenFile,
  onRetryDirectory,
  onContextMenu,
  revealRequest,
  onReveal,
}: Props) {
  const { t } = useTranslation("chat");
  const treeRef = useRef<HTMLDivElement>(null);
  const dragFromRow = useRef(true);
  const [focusedKey, setFocusedKey] = useState("");
  const activeKey =
    rows.find((row) => row.key === focusedKey)?.key ??
    rows.find((row) => row.entry.path === selectedPath)?.key ??
    rows[0]?.key;
  useEffect(() => {
    if (!revealRequest) return;
    const index =
      revealRequest.path === ""
        ? 0
        : rows.findIndex((row) => row.entry.path === revealRequest.path);
    const element = treeRef.current?.querySelectorAll<HTMLElement>('[role="treeitem"]')[index];
    if (!element) return;
    onReveal(element);
  }, [rows, revealRequest, onReveal]);
  const focusRow = (index: number) => {
    treeRef.current?.querySelectorAll<HTMLElement>('[role="treeitem"]')[index]?.focus();
  };
  return (
    <div
      ref={treeRef}
      onPointerDownCapture={(event) => {
        // Native dragstart targets the draggable row, even when the press began on More.
        dragFromRow.current = !(event.target instanceof Element && event.target.closest("button"));
      }}
      aria-label={t("workspaceFilesTitle", { defaultValue: "Files" })}
      role="tree"
      className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden py-1"
    >
      {rows.map((node, index) => {
        const { entry } = node;
        const selected = !entry.is_dir && entry.path === selectedPath;
        const unavailable =
          entry.unavailable === "missing"
            ? entry.is_symlink
              ? t("workspaceLinkMissing", { defaultValue: "Link target not found" })
              : t("workspaceEntryMissing", { defaultValue: "Item no longer exists" })
            : entry.unavailable === "outside_scope"
              ? t("workspaceLinkOutside", { defaultValue: "Link target is outside this workspace" })
              : entry.unavailable === "unreadable"
                ? t("workspaceEntryUnreadable", { defaultValue: "Cannot access this item" })
                : entry.unavailable === "unsupported"
                  ? t("workspaceEntryUnsupported", { defaultValue: "Not a regular file or folder" })
                  : "";
        const Icon = entry.is_symlink
          ? entry.is_dir
            ? FolderSymlink
            : FileSymlink
          : entry.is_dir
            ? Folder
            : File;
        const activate = () => {
          if (entry.unavailable) return;
          if (entry.is_dir) onToggleDirectory(entry.path);
          else onOpenFile(entry.path);
        };
        return (
          <Fragment key={node.key}>
            <div
              role="treeitem"
              draggable={!!onDragEntry}
              onDragStart={(event) => {
                if (!dragFromRow.current) {
                  event.preventDefault();
                  return;
                }
                onDragEntry?.(event, entry);
              }}
              data-workspace-directory={
                entry.is_dir && !entry.unavailable
                  ? entry.path
                  : entry.path.slice(0, Math.max(0, entry.path.lastIndexOf("/")))
              }
              tabIndex={node.key === activeKey ? 0 : -1}
              aria-level={node.depth + 1}
              onFocus={() => setFocusedKey(node.key)}
              onKeyDown={(event) => {
                if (event.target !== event.currentTarget) return;
                if (event.key === "ArrowDown") focusRow(Math.min(rows.length - 1, index + 1));
                else if (event.key === "ArrowUp") focusRow(Math.max(0, index - 1));
                else if (event.key === "Home") focusRow(0);
                else if (event.key === "End") focusRow(rows.length - 1);
                else if (event.key === "ArrowRight") {
                  if (entry.is_dir && !node.expanded) activate();
                  else if (entry.is_dir && rows[index + 1]?.depth > node.depth) focusRow(index + 1);
                } else if (event.key === "ArrowLeft") {
                  if (entry.is_dir && node.expanded) onToggleDirectory(entry.path);
                  else {
                    for (let parent = index - 1; parent >= 0; parent--) {
                      if (rows[parent].depth < node.depth) {
                        focusRow(parent);
                        break;
                      }
                    }
                  }
                } else if (event.key === "Enter" || event.key === " ") {
                  activate();
                } else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
                  const rect = event.currentTarget.getBoundingClientRect();
                  onContextMenu(entry, rect.left, rect.bottom, event.currentTarget);
                } else return;
                event.preventDefault();
              }}
              aria-expanded={entry.is_dir ? node.expanded : undefined}
              aria-selected={selected}
              title={
                entry.ignored
                  ? entry.path + " · " + t("workspaceGitIgnored", { defaultValue: "Git ignored" })
                  : entry.path
              }
              style={{ paddingLeft: 8 + node.depth * 12 }}
              onClick={activate}
              onContextMenu={(event) => {
                event.preventDefault();
                onContextMenu(entry, event.clientX, event.clientY, event.currentTarget);
              }}
              className={classNames(
                // Roomier rows on phones, where these are touch targets rather than mouse targets.
                "group/row flex w-full items-center gap-1 py-2 pr-2 text-left text-[13px] outline-none transition-colors focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-[var(--color-border-focus)] sm:py-[3px]",
                dropTarget === entry.path &&
                  entry.is_dir &&
                  "ring-1 ring-inset ring-[var(--color-border-focus)]",
                entry.unavailable ? "cursor-default" : "cursor-pointer",
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
              <span
                className="flex w-4 shrink-0 items-center justify-center opacity-60"
                title={
                  entry.is_symlink
                    ? t("workspaceSymbolicLink", { defaultValue: "Symbolic link" })
                    : undefined
                }
              >
                <Icon aria-hidden="true" className="h-3.5 w-3.5" />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate">{entry.name}</span>
                {unavailable && (
                  <span className="block break-words text-xs text-[var(--color-text-secondary)]">
                    {unavailable}
                  </span>
                )}
              </span>
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
              <button
                type="button"
                tabIndex={node.key === activeKey ? 0 : -1}
                aria-label={t("workspaceEntryActions", {
                  name: entry.name,
                  defaultValue: "Actions for {{name}}",
                })}
                aria-haspopup="menu"
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md hover:bg-[var(--glass-tab-bg)] focus-visible:opacity-100 sm:h-5 sm:w-5 sm:opacity-0 sm:group-hover/row:opacity-100 sm:group-focus-within/row:opacity-100"
                onClick={(event) => {
                  event.stopPropagation();
                  const rect = event.currentTarget.getBoundingClientRect();
                  onContextMenu(entry, rect.left, rect.bottom, event.currentTarget);
                }}
              >
                <MoreHorizontal aria-hidden="true" className="h-4 w-4" />
              </button>
            </div>
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
