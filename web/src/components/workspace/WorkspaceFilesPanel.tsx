import { SidePanelButton, SidePanelHeader } from "../layout/SidePanelHeader";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { EyeOff, RefreshCw } from "lucide-react";
import { classNames } from "../../utils/classNames";
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

type MenuState = { entry: WorkspaceEntry; x: number; y: number } | null;

export function WorkspaceFilesPanel({
  files,
  isDark,
  readOnly,
  onClose,
  onAttachPath,
  onPinPath,
}: Props) {
  const { t } = useTranslation("chat");
  const [menu, setMenu] = useState<MenuState>(null);

  const openContextMenu = useCallback((entry: WorkspaceEntry, x: number, y: number) => {
    setMenu({ entry, x, y });
  }, []);

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
    const absolute = files.rootPath ? `${files.rootPath}/${entry.path}` : entry.path;
    const items: WorkspaceMenuItem[] = [
      {
        key: "attach",
        label: t("workspaceAttachContext", { defaultValue: "Attach as context" }),
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
    if (!entry.is_dir && onPinPath && !readOnly) {
      items.push({
        key: "pin",
        label: t("workspacePinToSlot", { defaultValue: "Pin to a Presentation slot" }),
        onSelect: () => onPinPath(entry.path),
      });
    }
    return items;
  };

  const rootDirectory = files.tree.directories[""];
  const rootError = files.scopeAvailable
    ? rootDirectory?.error || ""
    : t("workspaceNoScope", { defaultValue: "Attach a workspace to this Group to browse files." });

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <SidePanelHeader
        title={t("workspaceFilesTitle", { defaultValue: "Files" })}
        subtitle={files.rootPath || undefined}
        onClose={onClose}
        closeLabel={t("workspaceClose", { defaultValue: "Close files" })}
      >
        <SidePanelButton
          title={t("workspaceShowIgnored", { defaultValue: "Show git-ignored files" })}
          onClick={() => files.setShowIgnored(!files.showIgnored)}
          aria-pressed={files.showIgnored}
          className={files.showIgnored ? "bg-[var(--glass-tab-bg)]" : undefined}
        >
          <EyeOff />
        </SidePanelButton>
        <SidePanelButton
          title={t("workspaceRefresh", { defaultValue: "Refresh" })}
          onClick={files.refresh}
        >
          <RefreshCw />
        </SidePanelButton>
      </SidePanelHeader>

      {/* A failed open clears the viewer, so its reason has to live next to the tree. */}
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
          rows={files.rows}
          selectedPath={files.selectedPath}
          isDark={isDark}
          onToggleDirectory={files.toggleDirectory}
          onOpenFile={openFile}
          onRetryDirectory={files.retryDirectory}
          onContextMenu={openContextMenu}
        />
      )}

      {menu ? (
        <WorkspaceEntryMenu
          x={menu.x}
          y={menu.y}
          items={menuItems(menu.entry)}
          isDark={isDark}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
}
