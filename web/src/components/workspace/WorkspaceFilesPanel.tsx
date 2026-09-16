import { SidePanelButton, SidePanelHeader } from "../layout/SidePanelHeader";
import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight, MoreHorizontal, RefreshCw } from "lucide-react";
import { classNames } from "../../utils/classNames";
import { workspaceContentUrl } from "../../services/api/workspace";
import { WorkspaceChangesPanel } from "./WorkspaceChangesPanel";
import { useWorkspaceActions } from "./useWorkspaceActions";
import { workspaceAbsolutePath, workspaceRelativePath } from "./workspacePath";
import type { WorkspaceEntry } from "../../types";
import { WorkspaceEntryMenu, type WorkspaceMenuItem } from "./WorkspaceEntryMenu";
import { WorkspaceTree } from "./WorkspaceTree";
import { WorkspaceFilesPlaceholder } from "./WorkspaceFilesPlaceholder";
import type { WorkspaceFilesController } from "./useWorkspaceFiles";

type Props = {
  /**
   * Owned by the chat shell, because the opened file renders in the main area while this
   * panel keeps showing the tree.
   */
  files: WorkspaceFilesController;
  isDark: boolean;
  readOnly: boolean;
  onClose: () => void;
  /** Drops a workspace-relative path into the composer so an Actor can act on it. */
  onAttachPath: (path: string) => void;
  /** Pins a workspace file to a Presentation slot, when the group allows it. */
  onPinPath?: (path: string) => void;
};

type MenuState = { entry?: WorkspaceEntry; x: number; y: number; anchor: HTMLElement } | null;

export function WorkspaceFilesPanel(props: Props) {
  const { groupId, scopeKey, scopeUrl } = props.files;
  return <FilesPanel key={JSON.stringify([groupId, scopeKey, scopeUrl])} {...props} />;
}

function FilesPanel({ files, isDark, readOnly, onClose, onAttachPath, onPinPath }: Props) {
  const { t } = useTranslation("chat");
  const actions = useWorkspaceActions(files, readOnly || files.mode !== "files");
  const [menu, setMenu] = useState<MenuState>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const { completeReveal } = files;
  const revealRow = useCallback(
    (row: HTMLElement) => {
      // The user may have moved to the editor, composer or an overlay during the lookup.
      if (panelRef.current?.contains(document.activeElement)) {
        row.scrollIntoView({ block: "nearest" });
        row.focus({ preventScroll: true });
      }
      completeReveal();
    },
    [completeReveal],
  );

  const [pathInput, setPathInput] = useState("");
  const [pathError, setPathError] = useState("");

  const openContextMenu = useCallback(
    (entry: WorkspaceEntry, x: number, y: number, anchor: HTMLElement) => {
      setMenu({ entry, x, y, anchor });
    },
    [],
  );

  const openFile = useCallback(
    (path: string) => {
      void files.openFile(path);
    },
    [files],
  );

  const copy = useCallback((value: string) => {
    void navigator.clipboard?.writeText(value);
  }, []);

  const menuItems = (entry: WorkspaceEntry): WorkspaceMenuItem[] => {
    const absolute = workspaceAbsolutePath(files.rootPath, entry.path);
    const items: WorkspaceMenuItem[] = [
      {
        key: "attach",
        label: t("workspaceAttachContext", { defaultValue: "Attach as context" }),
        disabled: !!entry.unavailable,
        onSelect: () => onAttachPath(entry.path),
      },
      {
        key: "copy-relative",
        label: t("workspaceCopyRelativePath", { defaultValue: "Copy relative path" }),
        onSelect: () => copy(entry.path),
      },
      {
        key: "copy-absolute",
        label: t("workspaceCopyPath", { defaultValue: "Copy absolute path" }),
        onSelect: () => copy(absolute),
      },
    ];
    if (!entry.is_dir) {
      items.push({
        key: "download",
        label: t("workspaceDownload", { defaultValue: "Download" }),
        disabled: !!entry.unavailable,
        href: workspaceContentUrl(
          files.groupId,
          { path: entry.path, scope_key: files.scopeKey, scope_url: files.scopeUrl },
          true,
        ),
        download: entry.name,
      });
    }
    if (!entry.is_dir && onPinPath && !readOnly) {
      items.push({
        key: "pin",
        label: t("workspacePinToSlot", { defaultValue: "Pin to a Presentation slot" }),
        disabled: !!entry.unavailable,
        onSelect: () => onPinPath(entry.path),
      });
    }
    return [...items, ...actions.entryItems(entry)];
  };

  const rootDirectory = files.tree.directories[""];
  const rootError = files.scopeAvailable
    ? rootDirectory?.error || ""
    : t("workspaceNoScope", { defaultValue: "Attach a workspace to this Group to browse files." });

  return (
    <div
      {...actions.dragProps}
      ref={panelRef}
      className="flex h-full min-h-0 min-w-0 flex-col"
      onPointerDownCapture={completeReveal}
      onKeyDownCapture={completeReveal}
    >
      <SidePanelHeader
        title={t("workspaceFilesTitle", { defaultValue: "Files" })}
        subtitle={files.rootPath || files.scopeUrl || undefined}
        onClose={onClose}
        closeLabel={t("workspaceClose", { defaultValue: "Close files" })}
      >
        <SidePanelButton
          title={t("workspaceRefresh", { defaultValue: "Refresh directory" })}
          onClick={files.refresh}
          disabled={!files.scopeAvailable}
        >
          <RefreshCw />
        </SidePanelButton>
        {files.mode === "files" && (
          <SidePanelButton
            title={t("workspaceOptions", { defaultValue: "File browser options" })}
            aria-haspopup="menu"
            aria-expanded={!!menu && !menu.entry}
            onClick={(event) => {
              const anchor = event.currentTarget;
              const rect = anchor.getBoundingClientRect();
              setMenu({ anchor, x: rect.left, y: rect.bottom });
            }}
          >
            <MoreHorizontal />
          </SidePanelButton>
        )}
      </SidePanelHeader>

      <div
        className="flex shrink-0 gap-1 border-b border-[var(--glass-border-subtle)] px-2 pb-2"
        role="group"
        aria-label={t("workspaceGit.view")}
      >
        {(["files", "changes"] as const).map((mode) => (
          <button
            key={mode}
            type="button"
            aria-pressed={files.mode === mode}
            className="rounded-md px-3 py-1 text-xs hover:bg-[var(--glass-tab-bg)] aria-pressed:bg-[var(--glass-tab-bg)] aria-pressed:font-semibold"
            onClick={() => {
              setMenu(null);
              files.setMode(mode);
            }}
          >
            {t(mode === "files" ? "workspaceFilesTitle" : "workspaceGit.changes")}
          </button>
        ))}
      </div>
      {files.mode === "changes" ? (
        <WorkspaceChangesPanel files={files} />
      ) : (
        <>
          {actions.controls}
          <form
            className="flex shrink-0 gap-1 border-b border-[var(--glass-border-subtle)] p-2"
            onSubmit={(event) => {
              event.preventDefault();
              const path = workspaceRelativePath(pathInput, files.rootPath);
              if (path === null) {
                setPathError(
                  t("workspacePathInvalid", {
                    defaultValue: "Enter a file or folder path inside this workspace.",
                  }),
                );
                return;
              }
              setPathError("");
              void files.locatePath(path);
            }}
          >
            <input
              type="text"
              value={pathInput}
              onChange={(event) => {
                setPathInput(event.target.value);
                setPathError("");
              }}
              aria-label={t("workspaceOpenPath", { defaultValue: "Go to file or folder" })}
              placeholder={t("workspaceOpenPath", { defaultValue: "Go to file or folder" })}
              disabled={!files.scopeAvailable}
              className="min-w-0 flex-1 rounded-md border border-[var(--glass-border-subtle)] bg-transparent px-2 py-1 text-xs outline-none focus:border-[var(--color-border-focus)]"
            />
            <SidePanelButton
              type="submit"
              title={t("workspaceOpenFile", { defaultValue: "Go" })}
              disabled={!files.scopeAvailable || !pathInput.trim()}
              aria-busy={files.pathLoading || files.fileLoading}
            >
              <ArrowRight />
            </SidePanelButton>
          </form>
          {(pathError || files.pathError) && (
            <p role="alert" className="px-3 py-2 text-xs text-rose-600">
              {pathError || files.pathError}
            </p>
          )}

          {/* With no open viewer, read errors belong next to the tree. */}
          {!files.file && files.fileError ? (
            <div
              className={classNames(
                "px-3 py-2 text-[12px]",
                isDark ? "bg-rose-400/10 text-rose-200" : "bg-rose-500/8 text-rose-700",
              )}
            >
              {files.fileError}
            </div>
          ) : null}

          {rootError ? (
            <div className="px-3 py-6 text-center text-[12px] opacity-60">{rootError}</div>
          ) : files.rows.length === 0 ? (
            <WorkspaceFilesPlaceholder
              loading={!rootDirectory || rootDirectory.loading}
              showIgnored={files.showIgnored}
              onShowIgnored={() => files.setShowIgnored(true)}
            />
          ) : (
            <WorkspaceTree
              onDragEntry={actions.startDrag}
              dropTarget={actions.dropTarget}
              rows={files.rows}
              selectedPath={files.selectedPath}
              isDark={isDark}
              onToggleDirectory={files.toggleDirectory}
              onOpenFile={openFile}
              onRetryDirectory={files.retryDirectory}
              onContextMenu={openContextMenu}
              revealRequest={files.revealRequest}
              onReveal={revealRow}
            />
          )}
        </>
      )}
      {menu ? (
        <WorkspaceEntryMenu
          x={menu.x}
          y={menu.y}
          anchor={menu.anchor}
          label={
            menu.entry
              ? menu.entry.name
              : t("workspaceOptions", { defaultValue: "File browser options" })
          }
          items={
            menu.entry
              ? menuItems(menu.entry)
              : [
                  ...actions.createItems(""),
                  {
                    key: "ignored",
                    label: t("workspaceHideIgnored", { defaultValue: "Hide Git-ignored files" }),
                    checked: !files.showIgnored,
                    onSelect: () => files.setShowIgnored(!files.showIgnored),
                  },
                  {
                    key: "reveal",
                    label: t("workspaceRevealFile", { defaultValue: "Reveal current file" }),
                    disabled: !files.file,
                    onSelect: () => {
                      if (files.file) files.revealFile(files.file.path);
                    },
                  },
                  {
                    key: "collapse",
                    label: t("workspaceCollapseAll", { defaultValue: "Collapse all folders" }),
                    disabled: !files.tree.expanded.length,
                    onSelect: files.collapseAll,
                  },
                ]
          }
          isDark={isDark}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
}
