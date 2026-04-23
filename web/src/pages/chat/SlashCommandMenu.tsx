import { classNames } from "../../utils/classNames";
import type { SlashCommandItem } from "../../utils/slashCommands";

export function SlashCommandMenu(props: {
  isDark: boolean;
  suggestions: SlashCommandItem[];
  selectedIndex: number;
  onSelect: (item: SlashCommandItem) => void;
  onHover: (index: number) => void;
}) {
  const { isDark, suggestions, selectedIndex, onSelect, onHover } = props;
  if (suggestions.length <= 0) return null;

  return (
    <div
      className={classNames(
        "glass-panel absolute bottom-full left-2 mb-3 w-72 max-h-60 overflow-auto scrollbar-subtle rounded-2xl border shadow-2xl z-30 animate-in fade-in zoom-in-95 duration-200",
      )}
      role="listbox"
    >
      {suggestions.map((item, idx) => (
        <button
          key={item.command}
          className={classNames(
            "w-full text-left px-4 py-3 text-sm transition-colors",
            isDark ? "text-slate-200 border-b border-white/5" : "text-gray-700 border-b border-black/5",
            idx === selectedIndex
              ? "bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] font-medium"
              : isDark ? "hover:bg-white/5" : "hover:bg-gray-50",
          )}
          onMouseDown={(e) => {
            e.preventDefault();
            onSelect(item);
          }}
          onMouseEnter={() => onHover(idx)}
        >
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0">
              <div className="truncate font-medium">{item.command}</div>
              {item.description ? (
                <div className={classNames("truncate text-[11px]", isDark ? "text-slate-400" : "text-gray-500")}>
                  {item.description}
                </div>
              ) : null}
            </div>
            <span className={classNames("flex-shrink-0 rounded-md px-2 py-0.5 text-[10px] uppercase tracking-wide", isDark ? "bg-white/8 text-slate-400" : "bg-black/5 text-gray-500")}>
              tool
            </span>
          </div>
        </button>
      ))}
    </div>
  );
}
