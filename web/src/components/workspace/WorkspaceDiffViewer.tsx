import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Diff, Hunk, parseDiff } from "react-diff-view";
import "react-diff-view/style/index.css";
import "./workspaceDiff.css";
import { SidePanelButton, SidePanelHeader } from "../layout/SidePanelHeader";
import { RefreshCw, File } from "lucide-react";
import type { WorkspaceFilesController } from "./useWorkspaceFiles";

export function WorkspaceDiffViewer({ files }: { files: WorkspaceFilesController }) {
  const { t } = useTranslation("chat");
  const { changes } = files;
  const container = useRef<HTMLDivElement>(null);
  const [wide, setWide] = useState(false);
  const [split, setSplit] = useState(false);
  useEffect(() => {
    const element = container.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => setWide(entry.contentRect.width >= 800));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const parsed = useMemo(() => {
    if (!changes.patch?.patch) return [];
    try {
      return parseDiff(changes.patch.patch);
    } catch {
      return null;
    }
  }, [changes.patch]);
  const selection = changes.selection;
  if (!selection) return null;
  const entry = changes.status?.entries.find((e) => e.path === selection.path);
  const canOpen = entry && entry.worktree !== "D" && entry.index !== "D";
  return (
    <div ref={container} className="workspace-diff flex min-h-0 min-w-0 flex-1 flex-col">
      <SidePanelHeader
        title={selection.path}
        subtitle={t(`workspaceGit.${selection.side}`)}
        onClose={changes.close}
        closeLabel={t("workspaceGit.closeDiff")}
      >
        {canOpen && (
          <SidePanelButton
            title={t("workspaceGit.openFile")}
            onClick={() => {
              files.setMode("files");
              void files.locatePath(selection.path);
            }}
          >
            <File />
          </SidePanelButton>
        )}
        <SidePanelButton
          title={t("workspaceRefresh")}
          onClick={files.refresh}
          disabled={changes.patchLoading}
        >
          <RefreshCw />
        </SidePanelButton>
      </SidePanelHeader>
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--glass-border-subtle)] px-3 py-2 text-xs">
        <span className="mr-auto text-[var(--color-text-secondary)]">
          {t(
            files.hasDraftsUnder(selection.path)
              ? "workspaceGit.unsaved"
              : "workspaceGit.savedOnly",
          )}
        </span>
        <button
          type="button"
          aria-pressed={!split || !wide}
          onClick={() => setSplit(false)}
          className="rounded px-2 py-1 aria-pressed:bg-[var(--glass-tab-bg)]"
        >
          {t("workspaceGit.unified")}
        </button>
        <button
          type="button"
          disabled={!wide}
          aria-pressed={split && wide}
          onClick={() => setSplit(true)}
          className="rounded px-2 py-1 aria-pressed:bg-[var(--glass-tab-bg)] disabled:opacity-40"
        >
          {t("workspaceGit.split")}
        </button>
      </div>
      <div
        className="min-h-0 flex-1 overflow-auto"
        tabIndex={0}
        aria-label={t("workspaceGit.diff")}
      >
        {changes.patchLoading ? (
          <p role="status" className="p-4 text-sm">
            {t("workspaceGit.loading")}
          </p>
        ) : changes.patchError ? (
          <p role="alert" className="break-words p-4 text-sm text-rose-600">
            {changes.patchError}
          </p>
        ) : changes.patch?.limited ? (
          <p className="p-4 text-sm">{t("workspaceGit.patchLimited")}</p>
        ) : !changes.patch?.patch ? (
          <p className="p-4 text-sm">{t("workspaceGit.noDiff")}</p>
        ) : parsed?.length && parsed.every((file) => file.hunks.length) ? (
          parsed.map((file, index) => (
            <Diff
              key={index}
              viewType={split && wide ? "split" : "unified"}
              diffType={file.type}
              hunks={file.hunks}
              gutterType="default"
            >
              {(hunks) => hunks.map((hunk) => <Hunk key={hunk.content} hunk={hunk} />)}
            </Diff>
          ))
        ) : (
          <pre className="p-4 text-xs">{changes.patch.patch}</pre>
        )}
      </div>
    </div>
  );
}
