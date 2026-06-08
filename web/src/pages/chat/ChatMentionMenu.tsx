import { useEffect, useRef } from "react";

import { classNames } from "../../utils/classNames";
import type { ComposerMentionSuggestion } from "./chatMentionSuggestions";

type ChatMentionMenuProps = {
  isDark: boolean;
  isSmallScreen: boolean;
  items: ComposerMentionSuggestion[];
  left: number;
  selectedIndex: number;
  onSelect: (item: ComposerMentionSuggestion) => void;
  onHover: (index: number) => void;
};

export function ChatMentionMenu({
  isDark,
  isSmallScreen,
  items,
  left,
  selectedIndex,
  onSelect,
  onHover,
}: ChatMentionMenuProps) {
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    optionRefs.current.length = items.length;
    const node = optionRefs.current[selectedIndex];
    node?.scrollIntoView({ block: "nearest" });
  }, [items.length, selectedIndex]);

  return (
    <div
      className={classNames(
        "glass-panel absolute bottom-full left-2 right-2 sm:left-auto sm:right-auto mb-3 w-auto sm:w-80 max-w-[calc(100vw-1rem)] max-h-60 overflow-auto scrollbar-subtle rounded-2xl border shadow-2xl z-30 animate-in fade-in zoom-in-95 duration-200",
      )}
      style={isSmallScreen ? undefined : { left: `${left}px` }}
      role="listbox"
    >
      {items.map((item, index) => {
        const selected = index === selectedIndex;
        return (
          <button
            ref={(node) => {
              optionRefs.current[index] = node;
            }}
            key={`${item.kind}:${item.value}`}
            className={classNames(
              "relative w-full text-left px-4 py-3 text-sm transition-colors outline-none",
              isDark ? "text-slate-200 border-b border-white/5" : "text-gray-700 border-b border-black/5",
              selected
                ? isDark
                  ? "bg-white/12 text-white ring-1 ring-inset ring-white/24"
                  : "bg-black/[0.045] text-gray-950 ring-1 ring-inset ring-black/15"
                : isDark ? "hover:bg-white/5" : "hover:bg-gray-50",
            )}
            aria-selected={selected}
            onMouseDown={(event) => {
              event.preventDefault();
              onSelect(item);
            }}
            onMouseEnter={() => onHover(index)}
          >
            {selected ? (
              <span
                className={classNames(
                  "absolute bottom-2 left-1 top-2 w-1 rounded-full",
                  isDark ? "bg-white/70" : "bg-gray-700",
                )}
                aria-hidden="true"
              />
            ) : null}
            <div className="flex items-center gap-2 min-w-0">
              <span className={classNames("opacity-70 flex-shrink-0 font-semibold", selected ? (isDark ? "text-white" : "text-gray-900") : "")}>
                {item.kind === "group" ? "#" : "@"}
              </span>
              <div className="min-w-0 flex-1">
                <div className="break-words text-sm leading-5">{item.label}</div>
                {item.description ? (
                  <div className={classNames("line-clamp-2 break-words text-[11px] leading-4", isDark ? "text-slate-400" : "text-gray-500")}>
                    {item.description}
                  </div>
                ) : null}
                {item.meta ? (
                  <div className={classNames("truncate text-[10px] leading-4 opacity-55", isDark ? "text-slate-500" : "text-gray-400")}>
                    {item.meta}
                  </div>
                ) : null}
              </div>
            </div>
          </button>
        );
      })}
    </div>
  );
}
