import { useCallback, useState } from "react";
import {
  FloatingPortal,
  autoUpdate,
  flip,
  offset,
  shift,
  useDismiss,
  useFloating,
  useInteractions,
  useRole,
} from "@floating-ui/react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GroupMeta } from "../../types";
import { classNames } from "../../utils/classNames";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { MoreIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { GroupMenuAction } from "./GroupMenuAction";
import { GroupStatusIndicator } from "./GroupStatusIndicator";

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
  onSelect,
  onWarm,
}: SortableGroupItemProps) {
  const gid = String(group.group_id || "");
  const [menuOpen, setMenuOpen] = useState(false);

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
  const { refs, floatingStyles, context } = useFloating({
    open: menuOpen,
    onOpenChange: setMenuOpen,
    placement: "bottom-end",
    middleware: [offset(8), flip({ padding: 12 }), shift({ padding: 12 })],
    whileElementsMounted: autoUpdate,
    strategy: "fixed",
  });
  const dismiss = useDismiss(context);
  const role = useRole(context, { role: "menu" });
  const { getFloatingProps } = useInteractions([dismiss, role]);
  const setItemActivatorRef = useCallback(
    (node: HTMLElement | null) => {
      setActivatorNodeRef(node);
      refs.setReference(node);
    },
    [refs, setActivatorNodeRef],
  );
  const setFloating = useCallback((node: HTMLElement | null) => refs.setFloating(node), [refs]);

  const handleContextMenu = (event: React.MouseEvent<HTMLElement>) => {
    if (!onMenuAction || !menuActionLabel) return;
    event.preventDefault();
    refs.setPositionReference({
      getBoundingClientRect: () => new DOMRect(event.clientX, event.clientY, 0, 0),
    });
    setMenuOpen(true);
  };

  const handleItemKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return;
    if (
      onMenuAction &&
      menuActionLabel &&
      (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10"))
    ) {
      event.preventDefault();
      refs.setPositionReference(event.currentTarget);
      setMenuOpen(true);
      return;
    }
    if (event.key === "Enter" || (dragDisabled && event.key === " ")) {
      event.preventDefault();
      onSelect();
      return;
    }
    listeners?.onKeyDown?.(event);
  };

  const actionMenu = onMenuAction && menuActionLabel && (
    <FloatingPortal>
      {menuOpen && (
        <div
          ref={setFloating}
          style={floatingStyles}
          {...getFloatingProps({ "aria-label": menuAriaLabel || menuActionLabel })}
          className="z-max min-w-[160px] rounded-xl p-1.5 shadow-2xl glass-panel"
        >
          <GroupMenuAction
            label={menuActionLabel}
            onClick={() => {
              setMenuOpen(false);
              onMenuAction();
            }}
          />
        </div>
      )}
    </FloatingPortal>
  );

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
          onContextMenu={handleContextMenu}
          onKeyDown={handleItemKeyDown}
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
        {actionMenu}
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
        ref={setItemActivatorRef}
        {...attributes}
        {...listeners}
        className={classNames(
          "w-full px-3 py-3 rounded-xl transition-all min-h-[48px] flex items-center gap-2 relative",
          !dragDisabled && "cursor-grab select-none active:cursor-grabbing",
          isDragging && "opacity-70 shadow-lg ring-2 ring-[rgb(143,163,187)]/24",
          isActive ? "glass-group-item-active" : "glass-group-item",
          isArchived && !isActive && "opacity-90",
        )}
        role="button"
        tabIndex={0}
        onClick={onSelect}
        onContextMenu={handleContextMenu}
        onKeyDown={handleItemKeyDown}
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
        {onMenuAction && menuActionLabel && (
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            label={menuAriaLabel || menuActionLabel}
            className={classNames(
              "shrink-0 text-[var(--color-text-tertiary)] opacity-70 hover:opacity-100 focus-visible:opacity-100 md:pointer-fine:hidden",
              menuOpen &&
                "bg-[var(--glass-tab-bg)] text-[var(--color-text-primary)] opacity-100 shadow-sm",
            )}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => {
              event.stopPropagation();
              refs.setPositionReference(event.currentTarget);
              setMenuOpen((current) => !current);
            }}
          >
            <MoreIcon size={16} />
          </IconButton>
        )}
      </div>
      {actionMenu}
    </div>
  );
}
