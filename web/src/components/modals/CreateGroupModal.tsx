import { useTranslation } from "react-i18next";
import { useEffect, useRef, useState } from "react";
import { DirItem, DirSuggestion } from "../../types";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useChatObscuringViewOnPhone } from "../../hooks/useChatObscuringView";
import { ArrowDownIcon, DownloadIcon, FileIcon, FolderIcon, HomeIcon } from "../Icons";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { CreateGroupDirectoryBrowser } from "./CreateGroupDirectoryBrowser";
import {
  directoryNameFromPath,
  driveSuggestions,
  suggestionLabelKey,
} from "./createGroupDirectoryModel";
import { ModalFrame } from "./ModalFrame";
import { ActorConfigTabs } from "./ActorConfigTabs";
import { LocalSessionsPicker } from "../../features/localSessions/LocalSessionsPicker";
import { useUIStore } from "../../stores";

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
  dirCreateError?: string;
  onClearDirCreateError?: () => void;
  onRetryDir?: () => void;
  creatingDirectory: boolean;
  onFetchDirContents: (path: string) => void;
  onCreateDirectory: (parent: string, name: string) => Promise<boolean>;
  onCreateGroup: () => void;
  onSelectGroup?: (id: string) => void;
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
  dirCreateError,
  onClearDirCreateError,
  onRetryDir,
  creatingDirectory,
  onFetchDirContents,
  onCreateDirectory,
  onCreateGroup,
  onSelectGroup,
  onClose,
  onCancelAndReset,
}: CreateGroupModalProps) {
  const { t } = useTranslation("modals");
  const [entryTab, setEntryTab] = useState("blank");
  const [existingBusy, setExistingBusy] = useState(false);
  const [pairing, setPairing] = useState(false);
  const isAdmin = useUIStore((state) => state.isWebAdmin === true && !state.webReadOnly);
  const existingAllowed = isAdmin && Boolean(onSelectGroup);
  const existingActive = existingAllowed && entryTab === "existing";
  useEffect(() => {
    if (!isOpen) {
      setEntryTab("blank");
      setExistingBusy(false);
      setPairing(false);
    }
  }, [isOpen]);
  useChatObscuringViewOnPhone(isOpen);
  const pathInputRef = useRef<HTMLInputElement>(null);
  const { modalRef } = useModalA11y(isOpen, onClose, { initialFocusRef: pathInputRef });
  if (!isOpen) return null;

  // By the kind the server sends, not the name: the name of home and the current folder is
  // translated for display.
  const renderDirSuggestionIcon = (suggestion: DirSuggestion) => {
    const iconClassName = "h-[1.05rem] w-[1.05rem]";
    switch (suggestion.icon) {
      case "home":
        return <HomeIcon className={iconClassName} />;
      case "download":
        return <DownloadIcon className={iconClassName} />;
      case "document":
        return <FileIcon className={iconClassName} />;
      case "current":
        return <ArrowDownIcon className={iconClassName} />;
      default:
        return <FolderIcon className={iconClassName} />;
    }
  };
  const suggestionLabel = (suggestion: DirSuggestion) => {
    const key = suggestionLabelKey(suggestion);
    return key ? t(key) : suggestion.name;
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
            {t(existingActive ? "createGroup.subtitleExisting" : "createGroup.subtitle")}
          </div>
        </div>
      }
      closeAriaLabel={t("common:close")}
      panelClassName={`w-full h-full sm:mt-16 sm:max-h-[calc(100vh-8rem)] ${existingActive ? "sm:h-[calc(100vh-8rem)] sm:w-[min(100vw-2rem,72rem)] sm:max-w-[72rem]" : "sm:h-auto sm:max-w-lg"}`}
      modalRef={modalRef}
      footerActions={
        existingActive ? undefined : (
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
        )
      }
    >
      <ActorConfigTabs
        fill
        hideTabs={existingActive && pairing}
        ariaLabel={t("createGroup.title")}
        activeId={existingActive ? "existing" : "blank"}
        onChange={setEntryTab}
        tabs={[
          {
            id: "blank",
            label: t("createGroup.blankStart"),
            disabledReason: existingActive && existingBusy ? t("createGroup.creating") : undefined,
            panel: (
              <div className="p-6 space-y-5 overflow-y-auto min-h-0 flex-1 bg-[var(--color-bg-primary)]">
                {dirSuggestions.length > 0 && !createGroupPath && (
                  <div>
                    <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                      {t("createGroup.quickSelect")}
                    </label>
                    <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
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
                              {suggestionLabel(s)}
                            </div>
                            <div className="text-xs truncate text-[var(--color-text-muted)]">
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
                        // Follow the path while the name is still the one derived from the
                        // previous path; a name the user typed stays.
                        if (
                          !createGroupName ||
                          createGroupName === directoryNameFromPath(createGroupPath)
                        ) {
                          setCreateGroupName(dirName);
                        }
                      }}
                      placeholder={t("createGroup.pathPlaceholder")}
                      ref={pathInputRef}
                    />
                    <Button
                      variant="secondary"
                      onClick={() => onFetchDirContents(createGroupPath || "~")}
                    >
                      {t("createGroup.browse")}
                    </Button>
                  </div>
                  <div className="mt-1 text-xs text-[var(--color-text-muted)]">
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
                    createError={dirCreateError}
                    onClearCreateError={onClearDirCreateError}
                    onRetry={onRetryDir}
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
            ),
          },
          {
            id: "existing",
            label: t("createGroup.fromExistingSessions"),
            disabledReason: !existingAllowed
              ? t("actors:runtimeSwitch.notAdmin")
              : busy === "create" || creatingDirectory
                ? t("createGroup.creating")
                : undefined,
            panel:
              existingAllowed && onSelectGroup ? (
                <LocalSessionsPicker
                  active={existingActive}
                  groupId={null}
                  isDark={isDark}
                  onClose={onCancelAndReset}
                  onSelectGroup={onSelectGroup}
                  onBusyChange={setExistingBusy}
                  onPairingChange={setPairing}
                />
              ) : null,
          },
        ]}
      />
    </ModalFrame>
  );
}
