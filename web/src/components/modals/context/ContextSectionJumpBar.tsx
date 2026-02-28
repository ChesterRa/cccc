import { useTranslation } from "react-i18next";
import { classNames } from "../../../utils/classNames";

interface ContextSectionJumpBarProps {
  isDark: boolean;
  onScrollToSection: (id: string) => void;
}

const SECTION_KEYS: Array<{ id: string; key: string }> = [
  { id: "context-tasks", key: "jumpBar.tasks" },
  { id: "context-agents", key: "jumpBar.agents" },
  { id: "context-overview", key: "jumpBar.overview" },
  { id: "context-vision", key: "jumpBar.vision" },
  { id: "context-project", key: "jumpBar.project" },
];

export function ContextSectionJumpBar({ isDark, onScrollToSection }: ContextSectionJumpBarProps) {
  const { t } = useTranslation("modals");
  return (
    <div
      className={classNames(
        "flex flex-wrap gap-2 rounded-xl border px-2 py-2 shadow-sm",
        isDark ? "border-slate-700/80 bg-slate-900/70" : "border-gray-200 bg-white/85"
      )}
    >
      {SECTION_KEYS.map((item) => (
        <button
          key={item.id}
          type="button"
          className={classNames(
            "px-2.5 py-1.5 rounded-lg border text-xs font-medium transition-colors",
            isDark
              ? "border-slate-700 bg-slate-800/70 text-slate-200 hover:bg-slate-700/80"
              : "border-gray-200 bg-gray-50 text-gray-700 hover:bg-gray-100"
          )}
          onClick={() => onScrollToSection(item.id)}
        >
          {t(item.key)}
        </button>
      ))}
    </div>
  );
}
