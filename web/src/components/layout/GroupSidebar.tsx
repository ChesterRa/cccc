import { useMemo } from "react";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  TouchSensor,
  useSensor,
  useSensors,
  DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { GroupMeta } from "../../types";
import { classNames } from "../../utils/classNames";
import { CloseIcon, FolderIcon, ChevronLeftIcon, ChevronRightIcon, PlusIcon } from "../Icons";
import { SortableGroupItem } from "./SortableGroupItem";

export interface GroupSidebarProps {
  orderedGroups: GroupMeta[];
  groupOrder: string[];
  selectedGroupId: string;
  isOpen: boolean;
  isCollapsed: boolean;
  isDark: boolean;
  readOnly?: boolean;
  onSelectGroup: (groupId: string) => void;
  onCreateGroup?: () => void;
  onClose: () => void;
  onToggleCollapse: () => void;
  onReorder: (fromIndex: number, toIndex: number) => void;
}

export function GroupSidebar({
  orderedGroups,
  groupOrder,
  selectedGroupId,
  isOpen,
  isCollapsed,
  isDark,
  readOnly,
  onSelectGroup,
  onCreateGroup,
  onClose,
  onToggleCollapse,
  onReorder,
}: GroupSidebarProps) {
  // Memoize sortable item IDs to avoid unnecessary re-renders
  const sortableIds = useMemo(
    () => orderedGroups.map((g) => String(g.group_id || "")),
    [orderedGroups]
  );

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    }),
    useSensor(TouchSensor, {
      activationConstraint: {
        delay: 200,
        tolerance: 5,
      },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    })
  );

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (over && active.id !== over.id) {
      const oldIndex = groupOrder.indexOf(String(active.id));
      const newIndex = groupOrder.indexOf(String(over.id));
      if (oldIndex !== -1 && newIndex !== -1) {
        onReorder(oldIndex, newIndex);
      }
    }
  };

  return (
    <>
      <aside
        className={classNames(
          "h-full flex flex-col glass-sidebar",
          "fixed md:relative z-40 transition-all duration-300 ease-out",
          isCollapsed ? "w-[60px]" : "w-[280px]",
          isOpen ? "translate-x-0" : "-translate-x-full",
          "md:translate-x-0"
        )}
      >
        {/* Header */}
        <div className={`p-4 border-b ${isDark ? "border-white/5" : "border-black/5"}`}>
          <div
            className={classNames(
              "flex items-center",
              isCollapsed ? "justify-center" : "justify-between"
            )}
          >
            <div className={classNames("flex items-center", isCollapsed ? "" : "gap-3")}>
              <div className={classNames(
                "w-9 h-9 rounded-xl flex items-center justify-center glass-btn",
                isDark ? "text-cyan-400" : "text-cyan-600"
              )}>
                <img src="/ui/logo.svg" alt="CCCC Logo" className="w-6 h-6 object-contain" />
              </div>
              {!isCollapsed && (
                <span className={`text-lg font-bold tracking-tight ${isDark ? "text-white" : "text-gray-900"}`}>CCCC</span>
              )}
            </div>

            {!isCollapsed && (
              <div className="flex items-center gap-2">
                {!readOnly && onCreateGroup && (
                  <button
                    className={classNames(
                      "text-xs px-4 py-2 rounded-xl font-medium transition-all min-h-[36px] glass-btn-accent",
                      isDark ? "text-cyan-300" : "text-cyan-700"
                    )}
                    onClick={onCreateGroup}
                    title="Create new working group"
                    aria-label="Create new working group"
                  >
                    + New
                  </button>
                )}
                {/* Collapse button - desktop only */}
                <button
                  className={classNames(
                    "hidden md:flex p-2 min-w-[36px] min-h-[36px] items-center justify-center rounded-xl transition-all glass-btn",
                    isDark ? "text-slate-400 hover:text-white" : "text-gray-500 hover:text-gray-900"
                  )}
                  onClick={onToggleCollapse}
                  aria-label="Collapse sidebar"
                  title="Collapse sidebar"
                >
                  <ChevronLeftIcon size={16} />
                </button>
                {/* Close button - mobile only */}
                <button
                  className={classNames(
                    "md:hidden p-2 min-w-[44px] min-h-[44px] flex items-center justify-center rounded-xl transition-all glass-btn",
                    isDark ? "text-slate-400 hover:text-white" : "text-gray-500 hover:text-gray-900"
                  )}
                  onClick={onClose}
                  aria-label="Close sidebar"
                >
                  <CloseIcon size={18} />
                </button>
              </div>
            )}
          </div>
        </div>

        {/* Collapsed: expand button and new button */}
        {isCollapsed && (
          <div className="p-2 flex flex-col items-center gap-2">
            <button
              className={classNames(
                "w-10 h-10 rounded-xl flex items-center justify-center transition-all glass-btn",
                isDark ? "text-slate-400 hover:text-white" : "text-gray-500 hover:text-gray-900"
              )}
              onClick={onToggleCollapse}
              aria-label="Expand sidebar"
              title="Expand sidebar"
            >
              <ChevronRightIcon size={18} />
            </button>
            {!readOnly && onCreateGroup && (
              <button
                className={classNames(
                  "w-10 h-10 rounded-xl flex items-center justify-center transition-all glass-btn-accent",
                  isDark ? "text-cyan-300" : "text-cyan-700"
                )}
                onClick={onCreateGroup}
                aria-label="Create new working group"
                title="Create new working group"
              >
                <PlusIcon size={18} />
              </button>
            )}
          </div>
        )}

        {/* Group list */}
        <div className={classNames(
          "flex-1 overflow-auto",
          isCollapsed ? "p-2" : "p-3"
        )}>
          {!isCollapsed && (
            <div className={`text-[10px] font-medium uppercase tracking-wider mb-3 px-2 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              Working Groups
            </div>
          )}

          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
          >
            <SortableContext
              items={sortableIds}
              strategy={verticalListSortingStrategy}
            >
              <div className={classNames(
                isCollapsed ? "flex flex-col items-center gap-2" : "space-y-1.5"
              )}>
                {orderedGroups.map((g) => {
                  const gid = String(g.group_id || "");
                  const active = gid === selectedGroupId;
                  return (
                    <SortableGroupItem
                      key={gid}
                      group={g}
                      isActive={active}
                      isDark={isDark}
                      isCollapsed={isCollapsed}
                      dragDisabled={!!readOnly}
                      onSelect={() => {
                        onSelectGroup(gid);
                        if (window.matchMedia("(max-width: 767px)").matches) onClose();
                      }}
                    />
                  );
                })}
              </div>
            </SortableContext>
          </DndContext>

          {/* Empty state */}
          {!orderedGroups.length && !isCollapsed && (
            <div className="p-6 text-center">
              <div className={classNames(
                "w-16 h-16 mx-auto mb-4 rounded-2xl flex items-center justify-center glass-card",
                isDark ? "text-slate-400" : "text-gray-400"
              )}>
                <FolderIcon size={32} />
              </div>
              <div className={`text-sm mb-2 font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>No working groups yet</div>
              <div className={`text-xs mb-5 max-w-[200px] mx-auto leading-relaxed ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                A working group is a collaboration space where multiple AI agents work together on a project.
              </div>
              {!readOnly && onCreateGroup && (
                <button
                  className={classNames(
                    "text-sm px-5 py-2.5 rounded-xl font-medium min-h-[44px] transition-all glass-btn-accent",
                    isDark ? "text-cyan-300" : "text-cyan-700"
                  )}
                  onClick={onCreateGroup}
                >
                  Create Your First Group
                </button>
              )}
            </div>
          )}
        </div>
      </aside>

      {/* Sidebar overlay for mobile */}
      {isOpen && (
        <div
          className="fixed inset-0 z-30 md:hidden glass-overlay animate-fade-in"
          onPointerDown={(e) => {
            if (e.target === e.currentTarget) onClose();
          }}
          aria-hidden="true"
        />
      )}
    </>
  );
}
