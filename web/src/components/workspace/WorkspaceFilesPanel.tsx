import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { EyeOff, RefreshCw, X } from "lucide-react";
import { classNames } from "../../utils/classNames";
import type { WorkspaceEntry } from "../../types";
import { WorkspaceEntryMenu, type WorkspaceMenuItem } from "./WorkspaceEntryMenu";
import { WorkspaceTree } from "./WorkspaceTree";
import type { WorkspaceFilesController } from "./useWorkspaceFiles";
import { baseName } from "./workspaceTreeModel";

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

  const rootError = files.scopeAvailable
    ? files.tree.directories[""]?.error || ""
    : t("workspaceNoScope", { defaultValue: "Attach a workspace to this Group to browse files." });

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        className={classNames(
          "flex items-center gap-1 border-b px-2 py-1.5",
          isDark ? "border-white/8" : "border-black/8",
        )}
      >
        <span
          className="min-w-0 flex-1 truncate text-[13px] font-medium"
          title={files.rootPath || undefined}
        >
          {files.rootPath
            ? baseName(files.rootPath)
            : t("workspaceFilesTitle", { defaultValue: "Files" })}
        </span>
        <button
          type="button"
          onClick={() => files.setShowIgnored(!files.showIgnored)}
          aria-pressed={files.showIgnored}
          title={t("workspaceShowIgnored", { defaultValue: "Show git-ignored files" })}
          className={classNames(
            "rounded-md p-1 transition-colors",
            files.showIgnored ? "opacity-100" : "opacity-40",
            isDark ? "hover:bg-white/8" : "hover:bg-black/5",
          )}
        >
          <EyeOff className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          onClick={files.refresh}
          title={t("workspaceRefresh", { defaultValue: "Refresh" })}
          className={classNames(
            "rounded-md p-1 transition-colors",
            isDark ? "hover:bg-white/8" : "hover:bg-black/5",
          )}
        >
          <RefreshCw className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          onClick={onClose}
          title={t("workspaceClose", { defaultValue: "Close files" })}
          className={classNames(
            "rounded-md p-1 transition-colors",
            isDark ? "hover:bg-white/8" : "hover:bg-black/5",
          )}
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

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
