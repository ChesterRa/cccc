import {
  DndContext,
  closestCenter,
  MouseSensor,
  TouchSensor,
  useSensor,
  useSensors,
  DragEndEvent,
} from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { useCallback } from "react";
import { GroupMeta } from "../../types";
import { SortableGroupItem } from "./SortableGroupItem";
import { getSidebarSensorActivationConstraints } from "./groupSidebarModel";

interface GroupSidebarSortableListProps {
  groups: GroupMeta[];
  section: "working" | "archived";
  selectedGroupId: string;
  isDark: boolean;
  isCollapsed: boolean;
  readOnly?: boolean;
  menuActionLabel?: string;
  menuAriaLabel?: string;
  /** Screen-reader instructions for a sortable row; replaces dnd-kit's default. */
  reorderInstructions?: string;
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
  reorderInstructions,
  onMenuAction,
  onReorderSection,
  onSelectGroup,
  onWarmGroup,
  onClose,
}: GroupSidebarSortableListProps) {
  // Viewport width does not identify the input device: a narrow desktop or a
  // tablet may still use a mouse. Both sensors stay registered so each input
  // gets the activation gesture that suits it.
  const activation = getSidebarSensorActivationConstraints();
  // No KeyboardSensor: the row keeps Enter/Space for selection, so dnd-kit's
  // pick-up gesture can never start. Keyboard reordering is Alt+Arrow on the
  // row instead, wired through onMoveBy below.
  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: activation.mouse }),
    useSensor(TouchSensor, { activationConstraint: activation.touch }),
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
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
      accessibility={
        reorderInstructions
          ? { screenReaderInstructions: { draggable: reorderInstructions } }
          : undefined
      }
    >
      <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
        <div className={isCollapsed ? "flex flex-col items-center gap-2" : "space-y-1"}>
          {groups.map((group, index) => {
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
                onMoveBy={(delta) => {
                  const target = index + delta;
                  if (target < 0 || target >= groups.length) return;
                  onReorderSection(section, index, target);
                }}
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
