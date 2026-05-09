import { useLayoutEffect, useRef } from "react";
import { classNames } from "../../utils/classNames";
import { slashCommandDisplayKind, type SlashCommandItem } from "../../utils/slashCommands";

export function SlashCommandMenu(props: {
  isDark: boolean;
  suggestions: SlashCommandItem[];
  selectedIndex: number;
  hasMore?: boolean;
  loadMoreLabel: string;
  onSelect: (item: SlashCommandItem) => void;
  onHover: (index: number) => void;
  onLoadMore?: () => void;
}) {
  const { isDark, suggestions, selectedIndex, hasMore = false, loadMoreLabel, onSelect, onHover, onLoadMore } = props;
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useLayoutEffect(() => {
    const item = itemRefs.current[selectedIndex];
    if (!item) return;
    item.scrollIntoView({ block: "nearest" });
    menuRef.current?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex, suggestions]);

  if (suggestions.length <= 0) return null;

  const handleScroll = () => {
    if (!hasMore || !onLoadMore) return;
    const menu = menuRef.current;
    if (!menu) return;
    const remaining = menu.scrollHeight - menu.scrollTop - menu.clientHeight;
    if (remaining <= 24) onLoadMore();
  };

  return (
    <div
      ref={menuRef}
      onScroll={handleScroll}
      className="glass-panel absolute bottom-full left-2 z-30 mb-3 max-h-60 w-72 overflow-auto rounded-2xl border shadow-2xl animate-in fade-in zoom-in-95 duration-200 scrollbar-subtle"
      role="listbox"
    >
      {suggestions.map((item, idx) => {
        const displayKind = slashCommandDisplayKind(item);
        const isSelected = idx === selectedIndex;
        return (
          <button
            key={`${item.sourceType}:${item.command}`}
            ref={(node) => {
              itemRefs.current[idx] = node;
            }}
            className={classNames(
              "relative w-full px-4 py-3 text-left text-sm outline-none transition-colors",
              isDark ? "border-b border-white/5 text-slate-200" : "border-b border-black/5 text-gray-700",
              isSelected
                ? isDark
                  ? "bg-white/10 font-medium text-white shadow-[inset_3px_0_0_rgba(203,213,225,0.9),inset_0_0_0_1px_rgba(255,255,255,0.14)]"
                  : "bg-slate-100 font-medium text-slate-950 shadow-[inset_3px_0_0_rgba(71,85,105,0.82),inset_0_0_0_1px_rgba(71,85,105,0.18)]"
                : isDark ? "hover:bg-white/5" : "hover:bg-gray-50",
            )}
            role="option"
            aria-selected={isSelected}
            onMouseDown={(event) => {
              event.preventDefault();
              onSelect(item);
            }}
            onMouseEnter={() => onHover(idx)}
          >
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="truncate font-medium">{item.command}</div>
                {item.description ? (
                  <div
                    className={classNames(
                      "truncate text-[11px]",
                      isSelected
                        ? isDark ? "text-slate-200" : "text-slate-600"
                        : isDark ? "text-slate-400" : "text-gray-500",
                    )}
                  >
                    {item.description}
                  </div>
                ) : null}
              </div>
              <span
                className={classNames(
                  "flex-shrink-0 rounded-md px-2 py-0.5 text-[10px] uppercase tracking-wide",
                  isSelected
                    ? isDark ? "bg-white/12 text-slate-100" : "bg-white text-slate-700 shadow-sm"
                    : displayKind === "command"
                    ? isDark ? "bg-sky-400/12 text-sky-200" : "bg-sky-50 text-sky-700"
                    : displayKind === "skill"
                    ? isDark ? "bg-emerald-400/12 text-emerald-200" : "bg-emerald-50 text-emerald-700"
                    : isDark ? "bg-white/8 text-slate-400" : "bg-black/5 text-gray-500",
                )}
              >
                {displayKind}
              </span>
            </div>
          </button>
        );
      })}
      {hasMore ? (
        <div className={classNames("px-4 py-2 text-center text-[11px]", isDark ? "text-slate-500" : "text-gray-400")}>
          {loadMoreLabel}
        </div>
      ) : null}
    </div>
  );
}
