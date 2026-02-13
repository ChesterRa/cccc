import { useTranslation } from "react-i18next";
import { classNames } from "../../../utils/classNames";

interface ContextSectionJumpBarProps {
  isDark: boolean;
  onScrollToSection: (id: string) => void;
}

const SECTION_KEYS: Array<{ id: string; key: string }> = [
  { id: "context-project", key: "jumpBar.project" },
  { id: "context-vision", key: "jumpBar.vision" },
  { id: "context-sketch", key: "jumpBar.sketch" },
  { id: "context-tasks", key: "jumpBar.tasks" },
  { id: "context-notes", key: "jumpBar.notes" },
  { id: "context-references", key: "jumpBar.references" },
];

export function ContextSectionJumpBar({ isDark, onScrollToSection }: ContextSectionJumpBarProps) {
  const { t } = useTranslation("modals");
  return (
    <div className="flex flex-wrap gap-2">
      {SECTION_KEYS.map((item) => (
        <button
          key={item.id}
          type="button"
          className={classNames(
            "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
            isDark ? "text-slate-200" : "text-gray-800"
          )}
          onClick={() => onScrollToSection(item.id)}
        >
          {t(item.key)}
        </button>
      ))}
    </div>
  );
}
