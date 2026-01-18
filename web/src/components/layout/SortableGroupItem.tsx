import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GroupMeta } from "../../types";
import { getGroupStatus, getGroupStatusLight } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { GripIcon } from "../Icons";

interface SortableGroupItemProps {
  group: GroupMeta;
  isActive: boolean;
  isDark: boolean;
  isCollapsed: boolean;
  onSelect: () => void;
}

export function SortableGroupItem({
  group,
  isActive,
  isDark,
  isCollapsed,
  onSelect,
}: SortableGroupItemProps) {
  const gid = String(group.group_id || "");

  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: gid });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  const status = isDark
    ? getGroupStatus(group.running ?? false, group.state)
    : getGroupStatusLight(group.running ?? false, group.state);

  if (isCollapsed) {
    // Collapsed mode: show only initial or icon
    const initial = (group.title || gid).charAt(0).toUpperCase();
    return (
      <div ref={setNodeRef} style={style} {...attributes}>
        <button
          className={classNames(
            "w-10 h-10 rounded-xl flex items-center justify-center transition-all",
            isDragging && "opacity-50 shadow-lg",
            isActive
              ? "glass-btn-accent glow-pulse"
              : "glass-btn hover:scale-105"
          )}
          onClick={onSelect}
          title={group.title || gid}
        >
          <span
            className={classNames(
              "text-sm font-semibold",
              isActive
                ? isDark ? "text-cyan-300" : "text-cyan-700"
                : isDark ? "text-slate-300" : "text-gray-600"
            )}
          >
            {initial}
          </span>
        </button>
      </div>
    );
  }

  return (
    <div
      ref={setNodeRef}
      style={style}
      {...attributes}
      className={classNames(
        "group/item relative",
        isDragging && "z-50"
      )}
    >
      <button
        className={classNames(
          "w-full text-left px-3 py-2.5 rounded-xl transition-all min-h-[44px] flex items-center gap-2",
          isDragging && "opacity-70 shadow-lg ring-2 ring-cyan-500/30",
          isActive
            ? "glass-btn-accent glow-pulse"
            : "glass-btn hover:translate-x-1"
        )}
        onClick={onSelect}
      >
        {/* Drag handle */}
        <div
          {...listeners}
          className={classNames(
            "flex-shrink-0 cursor-grab active:cursor-grabbing p-1 -ml-1 rounded transition-opacity touch-none",
            "opacity-0 group-hover/item:opacity-100",
            isDragging && "opacity-100",
            isDark ? "text-slate-500 hover:text-slate-300" : "text-gray-400 hover:text-gray-600"
          )}
          onClick={(e) => e.stopPropagation()}
        >
          <GripIcon size={14} />
        </div>

        <div className="flex-1 min-w-0 flex items-center justify-between">
          <div
            className={classNames(
              "text-sm font-medium truncate",
              isActive
                ? isDark ? "text-cyan-300" : "text-cyan-700"
                : isDark ? "text-slate-300 group-hover/item:text-white" : "text-gray-700 group-hover/item:text-gray-900"
            )}
          >
            {group.title || gid}
          </div>
          <div
            className={classNames(
              "text-[9px] px-2 py-0.5 rounded-full font-medium backdrop-blur-sm flex-shrink-0 ml-2",
              status.pillClass
            )}
          >
            {status.label}
          </div>
        </div>
      </button>
    </div>
  );
}
