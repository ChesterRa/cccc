import { AppWindow } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useGroupResourcePanel } from "../../stores/useResourcePanelStore";
import { classNames } from "../../utils/classNames";
import { LcdIcon } from "../Icons";

const triggerClass =
  "flex h-8 pointer-coarse:h-10 shrink-0 items-center justify-center gap-1 rounded-md px-2 text-sm font-medium tabular-nums hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2 focus-visible:outline-offset-2";

/**
 * Header rail entry: how many resources are open, and the one way to show or hide the panel.
 * It stays with no tab open, dimmed, and opens the empty panel that the next attachment opens
 * in.
 */
export function ResourcePanelTrigger({
  groupId,
  shown,
  onToggle,
}: {
  groupId: string;
  /** Whether the panel is on screen now, not only whether it is expanded. */
  shown: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation("chat");
  const open = useGroupResourcePanel(groupId).tabs.length;
  const label = shown
    ? t(open ? "resourcePanel.hide" : "resourcePanel.hideEmpty")
    : open
      ? t("resourcePanel.show", { open })
      : t("resourcePanel.showEmpty");
  return (
    <button
      type="button"
      onClick={onToggle}
      title={label}
      aria-label={label}
      aria-expanded={shown}
      data-resource-panel-toggle
      className={classNames(
        triggerClass,
        shown
          ? "bg-[var(--color-bg-primary)] text-[var(--color-text-primary)] ring-1 ring-inset ring-[var(--glass-tab-border-active)]"
          : open
            ? "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            : "text-[var(--color-text-tertiary)] opacity-60 hover:text-[var(--color-text-primary)] hover:opacity-100",
      )}
    >
      <LcdIcon icon={AppWindow} glyph="app-windows" className="h-[18px] w-[18px]" />
      <span>{open}</span>
    </button>
  );
}
