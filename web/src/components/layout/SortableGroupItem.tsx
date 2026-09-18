import { Archive, ArchiveRestore, Link2 } from "lucide-react";
import { GroupConnectionBadge } from "../../features/connect/GroupConnectionBadge";
import type { GroupConnectionCount } from "../../features/connect/protocol";
import { useCallback } from "react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GroupMeta } from "../../types";
import { classNames } from "../../utils/classNames";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { useGroupMenu, type GroupMenuActionItem } from "./useGroupMenu";
import { GroupStatusIndicator } from "./GroupStatusIndicator";
import { GroupItemMenuTrigger } from "./GroupItemMenuTrigger";

interface SortableGroupItemProps {
  group: GroupMeta;
  isActive: boolean;
  isDark: boolean;
  isCollapsed: boolean;
  isArchived?: boolean;
  dragDisabled?: boolean;
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
  /** Move this group one place up (-1) or down (1) in its section. */
  onMoveBy?: (delta: -1 | 1) => void;
  onSelect: () => void;
  onWarm?: () => void;
}

export function SortableGroupItem({
  group,
  isActive,
  isDark: _isDark,
  isCollapsed,
  isArchived = false,
  dragDisabled = false,
  menuActionLabel,
  menuAriaLabel,
  onMenuAction,
  runActions,
  trailingActions,
  connectionsLabel,
  connection,
  onOpenConnections,
  onMoveBy,
  onSelect,
  onWarm,
}: SortableGroupItemProps) {
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

  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: gid, disabled: dragDisabled });
  const style = { transform: CSS.Transform.toString(transform), transition };
  const status = getGroupStatusFromSource(group);
  const setItemActivatorRef = useCallback(
    (node: HTMLElement | null) => {
      setActivatorNodeRef(node);
    },
    [setActivatorNodeRef],
  );
  // The row overrides dnd-kit's keyboard listener so Enter and Space keep
  // selecting the group. Reordering from the keyboard therefore needs its own
  // entry: Alt with an arrow moves the row one place without a pick-up phase.
  const keyboardReorder = !!onMoveBy && !dragDisabled;
  const handleItemKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return;
    if (keyboardReorder && event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      onMoveBy(event.key === "ArrowUp" ? -1 : 1);
      return;
    }
    if (menu.onKeyDown(event)) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect();
    }
  };

  if (isCollapsed) {
    const initial = (group.title || gid).charAt(0).toUpperCase();
    return (
      <div ref={setNodeRef} style={style}>
        <button
          ref={setItemActivatorRef}
          {...attributes}
          {...listeners}
          className={classNames(
            "w-11 h-11 rounded-xl flex items-center justify-center transition-all relative",
            !dragDisabled && "cursor-grab select-none active:cursor-grabbing",
            isDragging && "opacity-50 shadow-lg",
            isActive ? "glass-group-item-active" : "glass-group-item hover:scale-105",
          )}
          onClick={onSelect}
          onContextMenu={menu.onContextMenu}
          onKeyDown={handleItemKeyDown}
          aria-keyshortcuts={keyboardReorder ? "Alt+ArrowUp Alt+ArrowDown" : undefined}
          onMouseEnter={onWarm}
          onFocus={onWarm}
          title={group.title || gid}
        >
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
        {menu.menu}
      </div>
    );
  }

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={classNames("group/item relative", isDragging && "z-50")}
    >
      <div
        // The row itself is the drag activator. The explicit role, tabIndex
        // and onKeyDown below deliberately override what dnd-kit spreads
        // here: Enter/Space keep selecting the group, and Alt+Arrow moves it.
        // dnd-kit's own aria-roledescription and aria-describedby survive the
        // override, so the row still announces itself as sortable.
        ref={setItemActivatorRef}
        {...attributes}
        {...listeners}
        className={classNames(
          "w-full px-3 py-3 rounded-xl transition-all min-h-[48px] flex items-center gap-2 relative",
          // The removed grip carried select-none; the row now hosts the touch
          // long press instead, so it needs the same protection. Without it a
          // hold over the group title starts the browser's own text selection
          // or iOS callout, which competes with the drag it is meant to begin.
          "select-none [-webkit-touch-callout:none]",
          // Without a handle, the cursor is the only affordance left that
          // tells a mouse user the row can be dragged. `.glass-group-item`
          // sets `cursor: pointer` from an unlayered rule, which outranks a
          // plain utility no matter the order, so this has to be important.
          !dragDisabled && "!cursor-grab active:!cursor-grabbing",
          isDragging && "opacity-70 shadow-lg ring-2 ring-[rgb(143,163,187)]/24",
          isActive ? "glass-group-item-active" : "glass-group-item",
          isArchived && !isActive && "opacity-90",
        )}
        role="button"
        tabIndex={0}
        onClick={onSelect}
        onContextMenu={menu.onContextMenu}
        onKeyDown={handleItemKeyDown}
        aria-keyshortcuts={keyboardReorder ? "Alt+ArrowUp Alt+ArrowDown" : undefined}
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
