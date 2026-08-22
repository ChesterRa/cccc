import { FormEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { DirItem, DirSuggestion } from "../../types";
import { FolderIcon, PlusIcon } from "../Icons";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { directoryNameFromPath } from "./createGroupDirectoryModel";

interface CreateGroupDirectoryBrowserProps {
  dirItems: DirItem[];
  currentDir: string;
  parentDir: string | null;
  driveLocations: DirSuggestion[];
  error?: string;
  creatingDirectory: boolean;
  onSelect: (path: string, name: string) => void;
  onFetch: (path: string) => void;
  onCreateDirectory: (parent: string, name: string) => Promise<boolean>;
}

export function CreateGroupDirectoryBrowser({
  dirItems,
  currentDir,
  parentDir,
  driveLocations,
  error,
  creatingDirectory,
  onSelect,
  onFetch,
  onCreateDirectory,
}: CreateGroupDirectoryBrowserProps) {
  const { t } = useTranslation("modals");
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [directoryName, setDirectoryName] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const directories = dirItems.filter((item) => item.is_dir);
  const open = (path: string, name = directoryNameFromPath(path)) => {
    onSelect(path, name);
    onFetch(path);
  };
  useEffect(() => {
    if (showCreateForm) inputRef.current?.focus();
  }, [showCreateForm]);
  useEffect(() => {
    setDirectoryName("");
    setShowCreateForm(false);
  }, [currentDir]);
  useEffect(() => {
    if (!error) return;
    setDirectoryName("");
    setShowCreateForm(false);
  }, [error]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!directoryName.trim() || creatingDirectory) return;
    if (await onCreateDirectory(currentDir, directoryName)) {
      setDirectoryName("");
      setShowCreateForm(false);
    }
  };

  return (
    <div
      className={`rounded-xl max-h-56 overflow-auto ${error ? "border border-rose-500/30 bg-rose-500/10" : "glass-panel"}`}
    >
      {driveLocations.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2 border-[var(--glass-border-subtle)]">
          <span className="text-xs text-[var(--color-text-muted)]">
            {t("createGroup.locations")}
          </span>
          {driveLocations.map((location) => (
            <Button
              key={location.path}
              type="button"
              variant="secondary"
              className="h-8 px-3 font-mono text-xs"
              onClick={() => open(location.path)}
            >
              {location.path}
            </Button>
          ))}
        </div>
      )}
      {error && (
        <div
          role="alert"
          className="border-b px-3 py-3 text-sm text-rose-600 dark:text-rose-400 border-rose-500/30"
        >
          {error}
        </div>
      )}
      {!error && (
        <>
          {currentDir && (
            <div className="flex min-w-0 items-center gap-2 border-b px-3 py-1.5 border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)]">
              <div
                className="min-w-0 flex-1 truncate font-mono text-xs text-[var(--color-text-muted)]"
                title={currentDir}
              >
                {currentDir}
              </div>
              <Button
                type="button"
                variant="ghost"
                className="h-8 shrink-0 gap-1.5 px-2 text-xs"
                onClick={() => setShowCreateForm(true)}
                disabled={creatingDirectory}
              >
                <PlusIcon size={15} />
                {t("createGroup.newFolder")}
              </Button>
            </div>
          )}
          {showCreateForm && (
            <form
              className="flex flex-col gap-2 border-b p-3 sm:flex-row border-[var(--glass-border-subtle)]"
              onSubmit={submit}
            >
              <Input
                ref={inputRef}
                value={directoryName}
                onChange={(event) => setDirectoryName(event.target.value)}
                placeholder={t("createGroup.folderNamePlaceholder")}
                aria-label={t("createGroup.folderName")}
                disabled={creatingDirectory}
              />
              <div className="flex shrink-0 gap-2">
                <Button
                  type="button"
                  variant="ghost"
                  className="flex-1 sm:flex-none"
                  onClick={() => {
                    setDirectoryName("");
                    setShowCreateForm(false);
                  }}
                  disabled={creatingDirectory}
                >
                  {t("common:cancel")}
                </Button>
                <Button
                  type="submit"
                  className="flex-1 sm:flex-none"
                  disabled={!directoryName.trim() || creatingDirectory}
                >
                  {creatingDirectory
                    ? t("createGroup.creatingFolder")
                    : t("createGroup.createFolder")}
                </Button>
              </div>
            </form>
          )}
          {parentDir && (
            <Button
              type="button"
              variant="ghost"
              className="w-full justify-start gap-2 rounded-none border-b px-3 py-2 text-left min-h-[44px] hover:bg-[var(--glass-tab-bg-hover)] border-[var(--glass-border-subtle)]"
              onClick={() => open(parentDir)}
            >
              <span className="text-[var(--color-text-muted)]">
                <FolderIcon size={16} />
              </span>
              <span className="text-sm text-[var(--color-text-muted)]">..</span>
            </Button>
          )}
          {directories.length === 0 && (
            <div className="px-3 py-4 text-center text-sm text-[var(--color-text-muted)]">
              {t("createGroup.noSubdirectories")}
            </div>
          )}
          {directories.map((item) => (
            <Button
              type="button"
              key={item.path}
              variant="ghost"
              className="w-full justify-start gap-2 rounded-none px-3 py-2 text-left min-h-[44px] hover:bg-[var(--glass-tab-bg-hover)]"
              onClick={() => open(item.path, item.name)}
            >
              <span className="text-[var(--color-text-secondary)]">
                <FolderIcon size={16} />
              </span>
              <span className="min-w-0 truncate text-sm text-[var(--color-text-secondary)]">
                {item.name}
              </span>
            </Button>
          ))}
        </>
      )}
    </div>
  );
}
