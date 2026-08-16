import { useTranslation } from "react-i18next";
import type { DirItem, DirSuggestion } from "../../types";
import { FolderIcon } from "../Icons";
import { Button } from "../ui/button";
import { directoryNameFromPath } from "./createGroupDirectoryModel";

interface CreateGroupDirectoryBrowserProps {
  dirItems: DirItem[];
  currentDir: string;
  parentDir: string | null;
  driveLocations: DirSuggestion[];
  error?: string;
  onSelect: (path: string, name: string) => void;
  onFetch: (path: string) => void;
}

export function CreateGroupDirectoryBrowser({
  dirItems,
  currentDir,
  parentDir,
  driveLocations,
  error,
  onSelect,
  onFetch,
}: CreateGroupDirectoryBrowserProps) {
  const { t } = useTranslation("modals");
  const directories = dirItems.filter((item) => item.is_dir);
  const open = (path: string, name = directoryNameFromPath(path)) => {
    onSelect(path, name);
    onFetch(path);
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
      {error ? (
        <div className="px-3 py-3 text-sm text-rose-600 dark:text-rose-400">{error}</div>
      ) : (
        <>
          {currentDir && (
            <div
              className="px-3 py-1.5 border-b text-xs font-mono truncate border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-muted)]"
              title={currentDir}
            >
              {currentDir}
            </div>
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
