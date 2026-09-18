import { Archive, ArchiveRestore, Link2 } from "lucide-react";
import { GroupConnectionBadge } from "../../features/connect/GroupConnectionBadge";
import type { GroupConnectionCount } from "../../features/connect/protocol";
import { GroupMeta } from "../../types";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { GroupItemMenuTrigger } from "./GroupItemMenuTrigger";
import { useGroupMenu, type GroupMenuActionItem } from "./useGroupMenu";
import { GroupStatusIndicator } from "./GroupStatusIndicator";

interface GroupSidebarItemProps {
  group: GroupMeta;
  isActive: boolean;
  isCollapsed: boolean;
  isArchived?: boolean;
  menuActionLabel?: string;
  menuAriaLabel?: string;
  onMenuAction?: () => void;
  /** Launch/pause/stop entries for this group; listed before the other actions. */
  runActions?: GroupMenuActionItem[];
  /** Destructive entries for this group; listed after the other actions. */
  trailingActions?: GroupMenuActionItem[];
  connectionsLabel?: string;
  connection?: GroupConnectionCount;
  onOpenConnections?: () => void;
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
  runActions,
  trailingActions,
  connectionsLabel,
  connection,
  onOpenConnections,
  onSelect,
  onWarm,
}: GroupSidebarItemProps) {
  const gid = String(group.group_id || "");
  const menu = useGroupMenu(menuAriaLabel || menuActionLabel || "", [
    ...(runActions ?? []),
    ...(onOpenConnections && connectionsLabel
      ? [{ label: connectionsLabel, icon: <Link2 size={15} />, onClick: onOpenConnections }]
      : []),
    ...(onMenuAction && menuActionLabel
      ? [
          {
            label: menuActionLabel,
            icon: isArchived ? <ArchiveRestore size={15} /> : <Archive size={15} />,
            onClick: onMenuAction,
          },
        ]
      : []),
    ...(trailingActions ?? []),
  ]);
  const status = getGroupStatusFromSource(group);

  if (isCollapsed) {
    const initial = (group.title || gid).charAt(0).toUpperCase();
    return (
      <button
        className={classNames(
          "w-11 h-11 rounded-xl flex items-center justify-center transition-all relative",
          isActive ? "glass-group-item-active" : "glass-group-item hover:scale-105",
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
            isActive
              ? "text-[rgb(35,36,37)] dark:text-white"
              : "text-[var(--color-text-secondary)]",
          )}
        >
          {initial}
        </span>
        <GroupStatusIndicator
          status={status}
          className="absolute -bottom-0.5 -right-0.5 ring-2 ring-[var(--color-bg-primary)]"
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
          isArchived && !isActive && "opacity-90",
        )}
        role="button"
        tabIndex={0}
        onClick={onSelect}
        onContextMenu={menu.onContextMenu}
        onKeyDown={(event) => {
          if (event.target !== event.currentTarget || menu.onKeyDown(event)) return;
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
            <GroupStatusIndicator status={status} />
            <span
              className={classNames(
                "text-sm font-medium truncate",
                isActive
                  ? "text-[rgb(35,36,37)] dark:text-white"
                  : "text-[var(--color-text-primary)] group-hover/item:text-[var(--color-text-primary)]",
              )}
            >
              {group.title || gid}
            </span>
          </div>
        </div>

        <GroupConnectionBadge connection={connection} onClick={onOpenConnections} />
        {menu.available && (
          <GroupItemMenuTrigger
            isActive={isActive}
            label={menuAriaLabel || menuActionLabel || connectionsLabel || ""}
            open={menu.open}
            onToggle={menu.toggle}
          />
        )}
      </div>
      {menu.menu}
    </div>
  );
}
