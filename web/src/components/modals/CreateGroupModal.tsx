import { useTranslation } from "react-i18next";
import { DirItem, DirSuggestion } from "../../types";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ArrowDownIcon, DownloadIcon, FileIcon, FolderIcon, HomeIcon } from "../Icons";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { CreateGroupDirectoryBrowser } from "./CreateGroupDirectoryBrowser";
import { directoryNameFromPath, driveSuggestions } from "./createGroupDirectoryModel";
import { ModalFrame } from "./ModalFrame";

export interface CreateGroupModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;

  dirSuggestions: DirSuggestion[];
  dirItems: DirItem[];
  currentDir: string;
  parentDir: string | null;
  showDirBrowser: boolean;

  createGroupPath: string;
  setCreateGroupPath: (path: string) => void;
  createGroupName: string;
  setCreateGroupName: (name: string) => void;

  dirBrowseError?: string;
  creatingDirectory: boolean;
  onFetchDirContents: (path: string) => void;
  onCreateDirectory: (parent: string, name: string) => Promise<boolean>;
  onCreateGroup: () => void;
  onClose: () => void;
  onCancelAndReset: () => void;
}

export function CreateGroupModal({
  isOpen,
  isDark,
  busy,
  dirSuggestions,
  dirItems,
  currentDir,
  parentDir,
  showDirBrowser,
  createGroupPath,
  setCreateGroupPath,
  createGroupName,
  setCreateGroupName,
  dirBrowseError,
  creatingDirectory,
  onFetchDirContents,
  onCreateDirectory,
  onCreateGroup,
  onClose,
  onCancelAndReset,
}: CreateGroupModalProps) {
  const { t } = useTranslation("modals");
  const { modalRef } = useModalA11y(isOpen, onClose);
  if (!isOpen) return null;

  const renderDirSuggestionIcon = (suggestion: DirSuggestion) => {
    const name = String(suggestion.name || "")
      .trim()
      .toLowerCase();
    const path = String(suggestion.path || "")
      .trim()
      .toLowerCase();
    const iconClassName = "h-[1.05rem] w-[1.05rem]";

    if (name.includes("home")) return <HomeIcon className={iconClassName} />;
    if (name.includes("desktop")) return <FolderIcon className={iconClassName} />;
    if (name.includes("download")) return <DownloadIcon className={iconClassName} />;
    if (name.includes("document")) return <FileIcon className={iconClassName} />;
    if (name.includes("current") || path.endsWith("/.cccc"))
      return <ArrowDownIcon className={iconClassName} />;
    return <FolderIcon className={iconClassName} />;
  };

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onCancelAndReset}
      titleId="create-group-title"
      title={
        <div>
          <div className="text-lg font-semibold text-[var(--color-text-primary)]">
            {t("createGroup.title")}
          </div>
          <div className="text-sm mt-1 text-[var(--color-text-muted)]">
            {t("createGroup.subtitle")}
          </div>
        </div>
      }
      closeAriaLabel={t("common:close")}
      panelClassName="w-full h-full sm:h-auto sm:max-w-lg sm:mt-16 sm:max-h-[calc(100vh-8rem)]"
      modalRef={modalRef}
      footerActions={
        <div className="flex flex-col-reverse gap-3 pb-2 sm:flex-row sm:justify-end">
          <Button
            type="button"
            variant="secondary"
            className="w-full sm:w-auto transition-all ease-spring duration-300"
            onClick={onCancelAndReset}
          >
            {t("common:cancel")}
          </Button>
          <Button
            type="button"
            className="w-full sm:w-auto font-semibold transition-all ease-spring duration-300"
            onClick={onCreateGroup}
            disabled={!createGroupPath.trim() || busy === "create"}
          >
            {busy === "create" ? t("createGroup.creating") : t("createGroup.createGroup")}
          </Button>
        </div>
      }
    >
      <div className="p-6 space-y-5 overflow-y-auto min-h-0 flex-1 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.92),rgba(255,255,255,0)_30%),linear-gradient(180deg,var(--color-bg-primary),var(--color-sidebar-bg))] dark:bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.05),rgba(255,255,255,0)_34%),linear-gradient(180deg,rgba(17,18,22,0.98),rgba(11,12,15,1))]">
        {dirSuggestions.length > 0 && !createGroupPath && (
          <div>
            <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
              {t("createGroup.quickSelect")}
            </label>
            <div className="grid grid-cols-2 gap-2">
              {dirSuggestions.slice(0, 6).map((s) => (
                <button
                  key={s.path}
                  className="flex items-center gap-3 px-3 py-2 rounded-xl transition-colors text-left min-h-[56px] glass-card"
                  onClick={() => {
                    setCreateGroupPath(s.path);
                    setCreateGroupName(directoryNameFromPath(s.path));
                    onFetchDirContents(s.path);
                  }}
                >
                  <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]">
                    {renderDirSuggestionIcon(s)}
                  </span>
                  <div className="min-w-0">
                    <div className="text-sm font-medium truncate text-[var(--color-text-secondary)]">
                      {s.name}
                    </div>
                    <div className="text-[10px] truncate text-[var(--color-text-muted)]">
                      {s.path}
                    </div>
                  </div>
                </button>
              ))}
            </div>
          </div>
        )}
        <div>
          <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
            {t("createGroup.projectDirectory")}
          </label>
          <div className="flex gap-2">
            <Input
              className="flex-1 font-mono"
              value={createGroupPath}
              onChange={(e) => {
                setCreateGroupPath(e.target.value);
                const dirName = directoryNameFromPath(e.target.value);
                if (!createGroupName || createGroupName === directoryNameFromPath(currentDir)) {
                  setCreateGroupName(dirName);
                }
              }}
              placeholder={t("createGroup.pathPlaceholder")}
              autoFocus
            />
            <Button variant="secondary" onClick={() => onFetchDirContents(createGroupPath || "~")}>
              {t("createGroup.browse")}
            </Button>
          </div>
          <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
            {t("createGroup.pathAutoCreateHint")}
          </div>
        </div>
        {showDirBrowser && (
          <CreateGroupDirectoryBrowser
            dirItems={dirItems}
            currentDir={currentDir}
            parentDir={parentDir}
            driveLocations={driveSuggestions(dirSuggestions)}
            error={dirBrowseError}
            creatingDirectory={creatingDirectory}
            onSelect={(path, name) => {
              setCreateGroupPath(path);
              setCreateGroupName(name);
            }}
            onFetch={onFetchDirContents}
            onCreateDirectory={onCreateDirectory}
          />
        )}
        <div>
          <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
            {t("createGroup.groupName")}
          </label>
          <Input
            value={createGroupName}
            onChange={(e) => setCreateGroupName(e.target.value)}
            placeholder={t("createGroup.groupNamePlaceholder")}
          />
        </div>
      </div>
    </ModalFrame>
  );
}
