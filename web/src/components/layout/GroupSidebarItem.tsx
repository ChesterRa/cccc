import { useState } from "react";
import { GroupMeta } from "../../types";
import { classNames } from "../../utils/classNames";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { MoreIcon } from "../Icons";

interface GroupSidebarItemProps {
  group: GroupMeta;
  isActive: boolean;
  isCollapsed: boolean;
  isArchived?: boolean;
  menuActionLabel?: string;
  menuAriaLabel?: string;
  onMenuAction?: () => void;
  onSelect: () => void;
  onWarm?: () => void;
}

export function GroupSidebarItem({
  group,
  isActive,
  isCollapsed,
  isArchived = false,
  menuActionLabel,
  menuAriaLabel,
  onMenuAction,
  onSelect,
  onWarm,
}: GroupSidebarItemProps) {
  const gid = String(group.group_id || "");
  const [menuOpen, setMenuOpen] = useState(false);
  const status = getGroupStatusFromSource(group);

  if (isCollapsed) {
    const initial = (group.title || gid).charAt(0).toUpperCase();
    return (
      <button
        className={classNames(
          "w-11 h-11 rounded-xl flex items-center justify-center transition-all relative",
          isActive ? "glass-group-item-active" : "glass-group-item hover:scale-105"
        )}
        onClick={onSelect}
        onMouseEnter={onWarm}
        onFocus={onWarm}
        title={group.title || gid}
      >
        {isActive && (
          <span className="absolute left-0 top-3 bottom-3 w-0.75 rounded-r bg-[rgb(35,36,37)] dark:bg-white animate-in slide-in-from-left-1 duration-200" />
        )}
        <span
          className={classNames(
            "text-sm font-semibold",
            isActive ? "text-[rgb(35,36,37)] dark:text-white" : "text-[var(--color-text-secondary)]"
          )}
        >
          {initial}
        </span>
        <span
          className={classNames(
            "absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full ring-2 ring-[var(--color-bg-primary)]",
            status.dotClass
          )}
        />
      </button>
    );
  }

  return (
    <div className="group/item relative">
      {isActive && (
        <span className="absolute left-1.5 top-3.5 bottom-3.5 w-1 rounded-full bg-[rgb(35,36,37)] dark:bg-white z-10 animate-in slide-in-from-left-1 duration-200" />
      )}
      <div
        className={classNames(
          "w-full pr-3 py-3 rounded-xl transition-all min-h-[48px] flex items-center gap-2 relative",
          isActive ? "glass-group-item-active pl-5.5" : "glass-group-item pl-3",
          isArchived && !isActive && "opacity-90"
        )}
        role="button"
        tabIndex={0}
        onClick={onSelect}
        onKeyDown={(event) => {
          if (event.key !== "Enter" && event.key !== " ") return;
          event.preventDefault();
          onSelect();
        }}
      >
        <div
          className="flex-1 min-w-0 flex items-center justify-between gap-2 text-left"
          onMouseEnter={onWarm}
          onFocus={onWarm}
        >
          <div className="flex items-center gap-2 min-w-0">
            <span className={classNames("w-2.5 h-2.5 rounded-full flex-shrink-0", status.dotClass)} />
            <span
              className={classNames(
                "text-sm font-medium truncate",
                isActive
                  ? "text-[rgb(35,36,37)] dark:text-white"
                  : "text-[var(--color-text-primary)] group-hover/item:text-[var(--color-text-primary)]"
              )}
            >
              {group.title || gid}
            </span>
          </div>
        </div>

        {onMenuAction && menuActionLabel && (
          <div className="relative shrink-0">
            <button
              type="button"
              className={classNames(
                "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-transparent bg-transparent transition-all duration-150",
                "text-[var(--color-text-tertiary)] opacity-0 md:group-hover/item:opacity-100 focus-visible:opacity-100",
                menuOpen && "opacity-100 bg-[var(--glass-tab-bg)] border-[var(--glass-border-subtle)] text-[var(--color-text-primary)] shadow-sm",
                !menuOpen && isActive && "opacity-100 text-[rgb(35,36,37)] dark:text-white",
                !menuOpen && "hover:bg-[var(--glass-tab-bg-hover)] hover:border-[var(--glass-border-subtle)] hover:text-[var(--color-text-primary)]"
              )}
              aria-label={menuAriaLabel || menuActionLabel}
              title={menuAriaLabel || menuActionLabel}
              onClick={(event) => {
                event.stopPropagation();
                setMenuOpen((prev) => !prev);
              }}
            >
              <MoreIcon size={16} />
            </button>
            {menuOpen && (
              <div className="absolute right-0 top-full z-20 mt-2 min-w-[160px] rounded-xl p-1.5 shadow-2xl glass-panel">
                <button
                  type="button"
                  className={classNames(
                    "w-full rounded-lg px-3 py-2.5 text-left text-sm transition-colors",
                    "text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg-hover)]"
                  )}
                  onClick={(event) => {
                    event.stopPropagation();
                    setMenuOpen(false);
                    onMenuAction();
                  }}
                >
                  {menuActionLabel}
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
