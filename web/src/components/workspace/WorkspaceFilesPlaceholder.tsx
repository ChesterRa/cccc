import { FolderOpen, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";

type Props = { loading: boolean; showIgnored: boolean; onShowIgnored: () => void };

export function WorkspaceFilesPlaceholder({ loading, showIgnored, onShowIgnored }: Props) {
  const { t } = useTranslation("chat");
  return (
    <div
      role="status"
      className="flex min-h-0 flex-1 flex-col items-center justify-center px-5 py-8 text-center"
    >
      {loading ? (
        <>
          <Loader2
            aria-hidden="true"
            className="mb-3 h-5 w-5 animate-spin text-[var(--color-text-tertiary)] motion-reduce:animate-none"
          />
          <p className="text-[13px] text-[var(--color-text-secondary)]">
            {t("workspaceLoading", { defaultValue: "Loading…" })}
          </p>
        </>
      ) : (
        <>
          <FolderOpen
            aria-hidden="true"
            className="mb-3 h-7 w-7 text-[var(--color-text-tertiary)]"
          />
          <p className="text-[13px] font-medium text-[var(--color-text-secondary)]">
            {t("workspaceEmpty", { defaultValue: "No files to display" })}
          </p>
          <p className="mt-1 max-w-64 text-[12px] leading-relaxed text-[var(--color-text-tertiary)]">
            {showIgnored
              ? t("workspaceEmptyHint", {
                  defaultValue: "Add files to this workspace, then refresh.",
                })
              : t("workspaceEmptyFilteredHint", {
                  defaultValue: "Files may be hidden by Git ignore rules.",
                })}
          </p>
          {!showIgnored ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-3 h-auto min-h-[44px] max-w-full whitespace-normal py-2"
              onClick={onShowIgnored}
            >
              {t("workspaceShowIgnored", { defaultValue: "Show git-ignored files" })}
            </Button>
          ) : null}
        </>
      )}
    </div>
  );
}
