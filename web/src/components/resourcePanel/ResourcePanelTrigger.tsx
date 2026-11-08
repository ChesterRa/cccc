import { useState } from "react";
import { AppWindow } from "lucide-react";
import { useTranslation } from "react-i18next";
import { type ClosedEntry, useGroupResourcePanel } from "../../stores/useResourcePanelStore";
import { classNames } from "../../utils/classNames";
import { LcdIcon } from "../Icons";
import { Popover, PopoverAnchor, PopoverContent } from "../ui/popover";
import { ResourceTabsList } from "./ResourceTabsList";

const triggerClass =
  "flex h-8 pointer-coarse:h-10 shrink-0 items-center justify-center gap-1 rounded-md px-2 text-sm font-medium tabular-nums hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2 focus-visible:outline-offset-2";

/**
 * Header rail entry: how many resources are open, and the one way to show or hide them. With
 * every tab closed it stays while something was closed recently, so a closed tab can come back.
 */
export function ResourcePanelTrigger({
  groupId,
  shown,
  onToggle,
  onRestore,
}: {
  groupId: string;
  /** Whether the panel is on screen now, not only whether it is expanded. */
  shown: boolean;
  onToggle: () => void;
  onRestore: (entry: ClosedEntry) => void;
}) {
  const { t } = useTranslation("chat");
  const panel = useGroupResourcePanel(groupId);
  const [listOpen, setListOpen] = useState(false);
  const icon = <LcdIcon icon={AppWindow} glyph="app-windows" className="h-[18px] w-[18px]" />;

  if (!panel.tabs.length) {
    if (!panel.closed.length) return null;
    return (
      <Popover open={listOpen} onOpenChange={setListOpen}>
        <PopoverAnchor asChild>
          <button
            type="button"
            onClick={() => setListOpen((value) => !value)}
            title={t("resourcePanel.recentlyClosed")}
            aria-label={t("resourcePanel.recentlyClosed")}
            aria-expanded={listOpen}
            aria-haspopup="listbox"
            data-resource-panel-toggle
            className={classNames(
              triggerClass,
              "text-[var(--color-text-tertiary)] opacity-60 hover:opacity-100",
            )}
          >
            {icon}
            <span>0</span>
          </button>
        </PopoverAnchor>
        <PopoverContent
          align="end"
          sideOffset={4}
          collisionPadding={8}
          className="rounded-xl p-0"
          data-lcd-menu
        >
          <ResourceTabsList
            tabs={[]}
            activeKey=""
            closed={panel.closed}
            onSelect={() => undefined}
            onClose={() => undefined}
            onRestore={onRestore}
            onDone={() => setListOpen(false)}
          />
        </PopoverContent>
      </Popover>
    );
  }

  const label = shown
    ? t("resourcePanel.hide")
    : t("resourcePanel.show", { open: panel.tabs.length });
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
          : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]",
      )}
    >
      {icon}
      <span>{panel.tabs.length}</span>
    </button>
  );
}
