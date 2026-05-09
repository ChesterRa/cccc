import { useTranslation } from "react-i18next";
import { DirItem, DirSuggestion } from "../../types";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ArrowDownIcon, DownloadIcon, FileIcon, FolderIcon, HomeIcon } from "../Icons";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

export interface CreateGroupModalProps {
  isOpen: boolean;
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
  onFetchDirContents: (path: string) => void;
  onCreateGroup: () => void;
  onClose: () => void;
  onCancelAndReset: () => void;
}

export function CreateGroupModal({
  isOpen,
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
  onFetchDirContents,
  onCreateGroup,
  onClose,
  onCancelAndReset,
}: CreateGroupModalProps) {
  const { t } = useTranslation("modals");
  const { modalRef } = useModalA11y(isOpen, onClose);
  if (!isOpen) return null;

  const renderDirSuggestionIcon = (suggestion: DirSuggestion) => {
    const name = String(suggestion.name || "").trim().toLowerCase();
    const path = String(suggestion.path || "").trim().toLowerCase();
    const iconClassName = "h-[1.05rem] w-[1.05rem]";

    if (name.includes("home")) return <HomeIcon className={iconClassName} />;
    if (name.includes("desktop")) return <FolderIcon className={iconClassName} />;
    if (name.includes("download")) return <DownloadIcon className={iconClassName} />;
    if (name.includes("document")) return <FileIcon className={iconClassName} />;
    if (name.includes("current") || path.endsWith("/.cccc")) return <ArrowDownIcon className={iconClassName} />;
    return <FolderIcon className={iconClassName} />;
  };

  return (
    <div
      className="fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in glass-overlay"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="create-group-title"
    >
      <div
        ref={modalRef}
        className="w-full h-full sm:h-auto sm:max-w-lg sm:mt-16 shadow-2xl animate-scale-in overflow-hidden flex flex-col sm:max-h-[calc(100vh-8rem)] rounded-none sm:rounded-2xl glass-modal"
      >
        <div className="px-6 py-4 border-b safe-area-inset-top border-[var(--glass-border-subtle)] glass-header">
          <div id="create-group-title" className="text-lg font-semibold text-[var(--color-text-primary)]">
            {t("createGroup.title")}
          </div>
          <div className="text-sm mt-1 text-[var(--color-text-muted)]">{t("createGroup.subtitle")}</div>
        </div>
        <div className="p-6 space-y-5 overflow-y-auto min-h-0 flex-1 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.92),rgba(255,255,255,0)_30%),linear-gradient(180deg,rgb(251,250,247),rgb(245,244,241))] dark:bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.05),rgba(255,255,255,0)_34%),linear-gradient(180deg,rgba(17,18,22,0.98),rgba(11,12,15,1))]">
          {dirSuggestions.length > 0 && !createGroupPath && (
            <div>
              <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("createGroup.quickSelect")}</label>
              <div className="grid grid-cols-2 gap-2">
                {dirSuggestions.slice(0, 6).map((s) => (
                  <button
                    key={s.path}
                    className="flex items-center gap-3 px-3 py-2 rounded-xl transition-colors text-left min-h-[56px] glass-card"
                    onClick={() => {
                      setCreateGroupPath(s.path);
                      setCreateGroupName(s.path.split("/").filter(Boolean).pop() || "");
                      onFetchDirContents(s.path);
                    }}
                  >
                    <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]">
                      {renderDirSuggestionIcon(s)}
                    </span>
                    <div className="min-w-0">
                      <div className="text-sm font-medium truncate text-[var(--color-text-secondary)]">{s.name}</div>
                      <div className="text-[10px] truncate text-[var(--color-text-muted)]">{s.path}</div>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}
          <div>
            <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("createGroup.projectDirectory")}</label>
            <div className="flex gap-2">
              <Input
                className="flex-1 font-mono"
                value={createGroupPath}
                onChange={(e) => {
                  setCreateGroupPath(e.target.value);
                  const dirName = e.target.value.split("/").filter(Boolean).pop() || "";
                  if (!createGroupName || createGroupName === currentDir.split("/").filter(Boolean).pop()) {
                    setCreateGroupName(dirName);
                  }
                }}
                placeholder={t("createGroup.pathPlaceholder")}
                autoFocus
              />
              <Button
                variant="secondary"
                onClick={() => onFetchDirContents(createGroupPath || "~")}
              >
                {t("createGroup.browse")}
              </Button>
            </div>
            <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
              {t("createGroup.pathAutoCreateHint")}
            </div>
          </div>
          {showDirBrowser && (
            <div className={`rounded-xl max-h-48 overflow-auto ${dirBrowseError ? "border border-rose-500/30 bg-rose-500/10" : "glass-panel"}`}>
              {dirBrowseError ? (
                <div className="px-3 py-3 text-sm text-rose-600 dark:text-rose-400">{dirBrowseError}</div>
              ) : (
                <>
                  {currentDir && (
                    <div className="px-3 py-1.5 border-b text-xs font-mono truncate border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-muted)]">
                      {currentDir}
                    </div>
                  )}
                  {parentDir && (
                    <button
                      className="w-full flex items-center gap-2 px-3 py-2 text-left border-b min-h-[44px] hover:bg-[var(--glass-tab-bg-hover)] border-[var(--glass-border-subtle)]"
                      onClick={() => {
                        onFetchDirContents(parentDir);
                        setCreateGroupPath(parentDir);
                        setCreateGroupName(parentDir.split("/").filter(Boolean).pop() || "");
                      }}
                    >
                      <span className="text-[var(--color-text-muted)]"><FolderIcon size={16} /></span>
                      <span className="text-sm text-[var(--color-text-muted)]">..</span>
                    </button>
                  )}
                  {dirItems.filter((d) => d.is_dir).length === 0 && (
                    <div className="px-3 py-4 text-center text-sm text-[var(--color-text-muted)]">{t("createGroup.noSubdirectories")}</div>
                  )}
                  {dirItems
                    .filter((d) => d.is_dir)
                    .map((item) => (
                      <button
                        key={item.path}
                        className="w-full flex items-center gap-2 px-3 py-2 text-left min-h-[44px] hover:bg-[var(--glass-tab-bg-hover)]"
                        onClick={() => {
                          setCreateGroupPath(item.path);
                          setCreateGroupName(item.name);
                          onFetchDirContents(item.path);
                        }}
                      >
                        <span className="text-[var(--color-text-secondary)]"><FolderIcon size={16} /></span>
                        <span className="text-sm text-[var(--color-text-secondary)]">{item.name}</span>
                      </button>
                    ))}
                </>
              )}
            </div>
          )}
          <div>
            <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("createGroup.groupName")}</label>
            <Input
              value={createGroupName}
              onChange={(e) => setCreateGroupName(e.target.value)}
              placeholder={t("createGroup.groupNamePlaceholder")}
            />
          </div>

        </div>
        <div className="px-6 py-4 border-t border-[var(--glass-border-subtle)] glass-header">
          <div className="flex gap-3">
            <Button
              className="flex-1"
              onClick={onCreateGroup}
              disabled={
                !createGroupPath.trim() ||
                busy === "create"
              }
            >
              {busy === "create" ? t("createGroup.creating") : t("createGroup.createGroup")}
            </Button>
            <Button
              variant="secondary"
              onClick={onCancelAndReset}
            >
              {t("common:cancel")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
