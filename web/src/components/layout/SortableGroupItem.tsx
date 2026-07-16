import { useCallback, useMemo, useRef, useState } from "react";
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
import { GripIcon } from "../Icons";

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
  const setReference = useCallback((node: HTMLElement | null) => refs.setReference(node), [refs]);
  const setFloating = useCallback((node: HTMLElement | null) => refs.setFloating(node), [refs]);
  const setCombinedActivatorRef = useCallback(
    (node: HTMLButtonElement | null) => {
      setActivatorNodeRef(node);
      setReference(node);
    },
    [setActivatorNodeRef, setReference],
  );
  const dragListeners = useMemo(() => listeners ?? {}, [listeners]);
  const pointerStartRef = useRef<{ x: number; y: number } | null>(null);
  const suppressMenuClickRef = useRef(false);

  if (isCollapsed) {
    const initial = (group.title || gid).charAt(0).toUpperCase();
    return (
      <div ref={setNodeRef} style={style} {...attributes}>
        <button
          className={classNames(
            "w-11 h-11 rounded-xl flex items-center justify-center transition-all relative",
            isDragging && "opacity-50 shadow-lg",
            isActive ? "glass-group-item-active" : "glass-group-item hover:scale-105",
          )}
          onClick={onSelect}
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
          {/* Status dot */}
          <span
            className={classNames(
              "absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full ring-2 ring-[var(--color-bg-primary)]",
              status.dotClass,
            )}
          />
        </button>
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
        className={classNames(
          "w-full px-3 py-3 rounded-xl transition-all min-h-[48px] flex items-center gap-2 relative",
          isDragging && "opacity-70 shadow-lg ring-2 ring-[rgb(143,163,187)]/24",
          isActive ? "glass-group-item-active" : "glass-group-item",
          isArchived && !isActive && "opacity-90",
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
        {onMenuAction && menuActionLabel ? (
          <button
            type="button"
            {...attributes}
            {...dragListeners}
            ref={setCombinedActivatorRef}
            className={classNames(
              "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-transparent bg-transparent transition-all duration-150 touch-none",
              "cursor-grab active:cursor-grabbing text-[var(--color-text-tertiary)] opacity-0 md:group-hover/item:opacity-100 focus-visible:opacity-100",
              menuOpen &&
                "opacity-100 bg-[var(--glass-tab-bg)] border-[var(--glass-border-subtle)] text-[var(--color-text-primary)] shadow-sm",
              !menuOpen && isActive && "opacity-100 text-[rgb(35,36,37)] dark:text-white",
              !menuOpen &&
                "hover:bg-[var(--glass-tab-bg-hover)] hover:border-[var(--glass-border-subtle)] hover:text-[var(--color-text-primary)]",
              isDragging && "!opacity-100",
            )}
            aria-label={menuAriaLabel || menuActionLabel}
            title={menuAriaLabel || menuActionLabel}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onPointerDown={(event: React.PointerEvent<HTMLButtonElement>) => {
              event.stopPropagation();
              pointerStartRef.current = { x: event.clientX, y: event.clientY };
              suppressMenuClickRef.current = false;
            }}
            onPointerMove={(event: React.PointerEvent<HTMLButtonElement>) => {
              const start = pointerStartRef.current;
              if (!start) return;
              const dx = Math.abs(event.clientX - start.x);
              const dy = Math.abs(event.clientY - start.y);
              if (dx > 4 || dy > 4) {
                suppressMenuClickRef.current = true;
              }
            }}
            onClick={(event: React.MouseEvent<HTMLButtonElement>) => {
              event.stopPropagation();
              if (suppressMenuClickRef.current) {
                suppressMenuClickRef.current = false;
                pointerStartRef.current = null;
                return;
              }
              pointerStartRef.current = null;
              setMenuOpen((prev) => !prev);
            }}
          >
            <GripIcon size={14} />
          </button>
        ) : !dragDisabled ? (
          <button
            type="button"
            {...dragListeners}
            {...attributes}
            ref={setActivatorNodeRef}
            className={classNames(
              "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-transparent bg-transparent transition-all duration-150 touch-none",
              "cursor-grab active:cursor-grabbing text-[var(--color-text-tertiary)] opacity-0 md:group-hover/item:opacity-100 focus-visible:opacity-100",
              "hover:bg-[var(--glass-tab-bg-hover)] hover:border-[var(--glass-border-subtle)] hover:text-[var(--color-text-primary)]",
              isActive && "opacity-100 text-[rgb(35,36,37)] dark:text-white",
              isDragging && "!opacity-100",
            )}
            aria-label="Drag group"
            onPointerDown={(event) => event.stopPropagation()}
          >
            <GripIcon size={14} />
          </button>
        ) : null}

        <div
          className="flex-1 min-w-0 flex items-center justify-between gap-2 text-left"
          onMouseEnter={onWarm}
          onFocus={onWarm}
        >
          <div className="flex items-center gap-2 min-w-0">
            {/* Status dot */}
            <span
              className={classNames("w-2.5 h-2.5 rounded-full flex-shrink-0", status.dotClass)}
            />
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
          <FloatingPortal>
            {menuOpen && (
              <div
                ref={setFloating}
                style={floatingStyles}
                {...getFloatingProps()}
                className="z-max min-w-[160px] rounded-xl p-1.5 shadow-2xl glass-panel"
              >
                <button
                  type="button"
                  className={classNames(
                    "w-full rounded-lg px-3 py-2.5 text-left text-sm transition-colors",
                    "text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg-hover)]",
                  )}
                  onClick={() => {
                    setMenuOpen(false);
                    onMenuAction();
                  }}
                >
                  {menuActionLabel}
                </button>
              </div>
            )}
          </FloatingPortal>
        )}
      </div>
    </div>
  );
}
