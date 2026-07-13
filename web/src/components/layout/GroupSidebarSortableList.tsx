import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  MouseSensor,
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
import { useCallback } from "react";
import { GroupMeta } from "../../types";
import { SortableGroupItem } from "./SortableGroupItem";

interface GroupSidebarSortableListProps {
  groups: GroupMeta[];
  section: "working" | "archived";
  selectedGroupId: string;
  isDark: boolean;
  isCollapsed: boolean;
  readOnly?: boolean;
  menuActionLabel?: string;
  menuAriaLabel?: string;
  onMenuAction?: (groupId: string) => void;
  onReorderSection: (section: "working" | "archived", fromIndex: number, toIndex: number) => void;
  onSelectGroup: (groupId: string) => void;
  onWarmGroup?: (groupId: string) => void;
  onClose: () => void;
}

export function GroupSidebarSortableList({
  groups,
  section,
  selectedGroupId,
  isDark,
  isCollapsed,
  readOnly,
  menuActionLabel,
  menuAriaLabel,
  onMenuAction,
  onReorderSection,
  onSelectGroup,
  onWarmGroup,
  onClose,
}: GroupSidebarSortableListProps) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(MouseSensor, { activationConstraint: { distance: 8 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 200, tolerance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const ids = groups.map((g) => String(g.group_id || ""));
      const oldIndex = ids.indexOf(String(active.id));
      const newIndex = ids.indexOf(String(over.id));
      if (oldIndex !== -1 && newIndex !== -1) {
        onReorderSection(section, oldIndex, newIndex);
      }
    },
    [groups, onReorderSection, section],
  );

  const sortableIds = groups.map((g) => String(g.group_id || ""));
  const isArchivedSection = section === "archived";

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
      <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
        <div className={isCollapsed ? "flex flex-col items-center gap-2" : "space-y-1"}>
          {groups.map((group) => {
            const gid = String(group.group_id || "");
            return (
              <SortableGroupItem
                key={gid}
                group={group}
                isActive={gid === selectedGroupId}
                isDark={isDark}
                isCollapsed={isCollapsed}
                isArchived={isArchivedSection}
                dragDisabled={!!readOnly}
                menuActionLabel={menuActionLabel}
                menuAriaLabel={
                  menuAriaLabel ? `${menuAriaLabel} · ${group.title || gid}` : undefined
                }
                onMenuAction={onMenuAction ? () => onMenuAction(gid) : undefined}
                onSelect={() => {
                  onSelectGroup(gid);
                  if (window.matchMedia("(max-width: 767px)").matches) onClose();
                }}
                onWarm={gid === selectedGroupId ? undefined : () => onWarmGroup?.(gid)}
              />
            );
          })}
        </div>
      </SortableContext>
    </DndContext>
  );
}
