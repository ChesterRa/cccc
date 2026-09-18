import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { FolderIcon } from "../Icons";

type Props = { active: boolean; onToggle: () => void };

/**
 * Work-area rail entry into the workspace file tree.
 *
 * It sits next to the other work controls rather than floating over the chat, so nothing overlaps
 * the conversation and the column toggle reads as part of the same rail.
 */
export function WorkspaceFilesTrigger({ active, onToggle }: Props) {
  const { t } = useTranslation("chat");
  const label = active
    ? t("workspaceHideFiles", { defaultValue: "Hide workspace files" })
    : t("workspaceOpenFiles", { defaultValue: "Show workspace files" });

  return (
    <button
      type="button"
      onClick={onToggle}
      title={label}
      aria-label={label}
      aria-expanded={active}
      data-workspace-files-toggle="true"
      className={classNames(
        "flex h-8 w-8 pointer-coarse:h-10 pointer-coarse:w-10 shrink-0 items-center justify-center gap-1.5 rounded-md px-2 text-sm font-medium hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2 focus-visible:outline-offset-2",
        active
          ? "bg-[var(--color-bg-primary)] text-[var(--color-text-primary)] ring-1 ring-inset ring-[var(--glass-tab-border-active)]"
          : "text-[var(--color-text-secondary)]",
      )}
    >
      <FolderIcon size={18} className="shrink-0" aria-hidden="true" />
    </button>
  );
}
