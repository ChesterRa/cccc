import { classNames } from "../../../utils/classNames";

interface ContextSectionJumpBarProps {
  isDark: boolean;
  onScrollToSection: (id: string) => void;
}

const SECTION_ITEMS: Array<{ id: string; label: string }> = [
  { id: "context-project", label: "PROJECT" },
  { id: "context-vision", label: "Vision" },
  { id: "context-sketch", label: "Sketch" },
  { id: "context-tasks", label: "Tasks" },
  { id: "context-notes", label: "Notes" },
  { id: "context-references", label: "References" },
];

export function ContextSectionJumpBar({ isDark, onScrollToSection }: ContextSectionJumpBarProps) {
  return (
    <div className="flex flex-wrap gap-2">
      {SECTION_ITEMS.map((item) => (
        <button
          key={item.id}
          type="button"
          className={classNames(
            "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
            isDark ? "text-slate-200" : "text-gray-800"
          )}
          onClick={() => onScrollToSection(item.id)}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
