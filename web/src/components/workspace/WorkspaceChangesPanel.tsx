import { useTranslation } from "react-i18next";
import { GitBranch, File, Folder } from "lucide-react";
import type { WorkspaceChange } from "../../services/api/workspace";
import type { WorkspaceFilesController } from "./useWorkspaceFiles";

export function WorkspaceChangesPanel({ files }: { files: WorkspaceFilesController }) {
  const { t } = useTranslation("chat");
  const { changes } = files;
  if (changes.loading)
    return (
      <p role="status" className="p-3 text-sm">
        {t("workspaceGit.loading")}
      </p>
    );
  if (changes.error)
    return (
      <p role="alert" className="break-words p-3 text-sm text-rose-600">
        {changes.error}
      </p>
    );
  if (!changes.status) return null;
  const status = changes.status;
  if (!status.repository)
    return (
      <p className="p-3 text-sm text-[var(--color-text-secondary)]">
        {t("workspaceGit.noRepository")}
      </p>
    );
  const sections = [
    { key: "conflicts", entries: status.entries.filter((e) => e.conflicted) },
    {
      key: "worktree",
      entries: status.entries.filter((e) => !e.conflicted && !e.untracked && e.worktree !== " "),
    },
    {
      key: "staged",
      entries: status.entries.filter((e) => !e.conflicted && !e.untracked && e.index !== " "),
    },
    { key: "untracked", entries: status.entries.filter((e) => e.untracked) },
  ] as const;
  const activate = (entry: WorkspaceChange, section: string) => {
    if (section === "staged" || section === "worktree") changes.open(entry.path, section);
    else {
      files.setMode("files");
      void files.locatePath(entry.path);
    }
  };
  return (
    <div className="min-h-0 flex-1 overflow-auto py-2">
      <p className="flex items-center gap-2 px-3 text-xs">
        <GitBranch className="h-3.5 w-3.5 shrink-0" />
        <span className="truncate" title={status.branch}>
          {status.branch}
        </span>
      </p>
      <p className="px-3 py-2 text-xs text-[var(--color-text-secondary)]">
        {t("workspaceGit.savedOnly")}
      </p>
      {status.limited && (
        <p role="status" className="px-3 py-2 text-xs text-amber-600">
          {t("workspaceGit.limited")}
        </p>
      )}
      {!status.entries.length && (
        <p className="px-3 py-4 text-sm text-[var(--color-text-secondary)]">
          {t("workspaceGit.clean")}
        </p>
      )}
      {sections
        .filter((section) => section.entries.length)
        .map((section) => (
          <section key={section.key}>
            <h3 className="px-3 pb-1 pt-3 text-xs font-semibold">
              {t(`workspaceGit.${section.key}`)}{" "}
              <span className="font-normal opacity-60">{section.entries.length}</span>
            </h3>
            {section.entries.map((entry) => (
              <button
                key={entry.path}
                type="button"
                title={entry.previous_path ? `${entry.previous_path} → ${entry.path}` : entry.path}
                onClick={() => activate(entry, section.key)}
                aria-pressed={
                  changes.selection?.path === entry.path && changes.selection.side === section.key
                }
                className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs hover:bg-[var(--glass-tab-bg)] aria-pressed:bg-[var(--glass-tab-bg)] sm:py-1.5"
              >
                {entry.directory ? (
                  <Folder className="h-3.5 w-3.5 shrink-0" />
                ) : (
                  <File className="h-3.5 w-3.5 shrink-0" />
                )}
                <span className="min-w-0 flex-1 truncate">{entry.path}</span>
                <span className="shrink-0 font-mono text-[var(--color-text-secondary)]">
                  {section.key === "staged" ? entry.index : entry.worktree}
                </span>
              </button>
            ))}
          </section>
        ))}
    </div>
  );
}
