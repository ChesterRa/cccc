import { useRef, useEffect } from "react";
import { Actor } from "../types";
import { classNames } from "../utils/classNames";

interface TabBarProps {
  actors: Actor[];
  activeTab: string; // "chat" or actor id
  onTabChange: (tab: string) => void;
  unreadChatCount: number;
  isDark: boolean;
  onAddAgent?: () => void;
  canAddAgent?: boolean;
}

export function TabBar({ actors, activeTab, onTabChange, unreadChatCount, isDark, onAddAgent, canAddAgent = true }: TabBarProps) {
  const tabBarRef = useRef<HTMLDivElement>(null);
  const activeTabRef = useRef<HTMLButtonElement>(null);

  // Auto-scroll active tab into view
  useEffect(() => {
    if (activeTabRef.current && tabBarRef.current) {
      const container = tabBarRef.current;
      const tab = activeTabRef.current;
      const containerRect = container.getBoundingClientRect();
      const tabRect = tab.getBoundingClientRect();

      if (tabRect.left < containerRect.left) {
        container.scrollLeft -= containerRect.left - tabRect.left + 16;
      } else if (tabRect.right > containerRect.right) {
        container.scrollLeft += tabRect.right - containerRect.right + 16;
      }
    }
  }, [activeTab]);

  return (
    <div
      ref={tabBarRef}
      className={classNames(
        "flex items-center gap-1.5 px-3 py-1.5 overflow-x-auto border-b sticky top-0 z-10 backdrop-blur-md",
        isDark ? "border-white/5 bg-slate-900/70" : "border-black/5 bg-white/70"
      )}
      style={{ scrollbarWidth: 'none', msOverflowStyle: 'none' }}
      role="tablist"
      aria-label="Navigation tabs"
    >
      {/* Chat Tab */}
      <button
        ref={activeTab === "chat" ? activeTabRef : null}
        onClick={() => onTabChange("chat")}
        className={classNames(
          "relative flex items-center gap-2 px-3 py-2 text-sm font-medium whitespace-nowrap transition-all rounded-lg",
          activeTab === "chat"
            ? isDark ? "bg-white/10 text-white" : "bg-black/5 text-gray-900"
            : isDark ? "text-slate-400 hover:text-slate-200 hover:bg-white/5" : "text-gray-500 hover:text-gray-700 hover:bg-black/5"
        )}
        role="tab"
        aria-selected={activeTab === "chat"}
      >
        <span>Chat</span>
        {unreadChatCount > 0 && (
          <span className={classNames(
            "text-[10px] px-1.5 py-0.5 rounded-full font-bold",
            isDark ? "bg-cyan-500/20 text-cyan-300" : "bg-cyan-100 text-cyan-700"
          )}>
            {unreadChatCount}
          </span>
        )}
      </button>

      {/* Separator */}
      <div className={`w-px h-4 flex-shrink-0 ${isDark ? "bg-white/10" : "bg-black/8"}`} />

      {/* Agent Tabs */}
      {actors.map((actor) => {
        const isActive = activeTab === actor.id;
        const isRunning = actor.running ?? actor.enabled ?? false;

        return (
          <button
            key={actor.id}
            ref={isActive ? activeTabRef : null}
            onClick={() => onTabChange(actor.id)}
            className={classNames(
              "relative flex items-center gap-2 px-3 py-2 text-sm font-medium whitespace-nowrap transition-all rounded-lg",
              isActive
                ? isDark ? "bg-white/10 text-white" : "bg-black/5 text-gray-900"
                : isDark ? "text-slate-400 hover:text-slate-200 hover:bg-white/5" : "text-gray-500 hover:text-gray-700 hover:bg-black/5"
            )}
            role="tab"
            aria-selected={isActive}
          >
            {/* Run Indicator */}
            <span
              className={classNames(
                "w-2 h-2 rounded-full transition-all",
                isRunning 
                  ? "bg-emerald-500" 
                  : isDark ? "bg-slate-600" : "bg-gray-300"
              )}
            />

            <span>{actor.id}</span>

            {actor.role === "foreman" && (
              <span className={classNames(
                "text-[9px] px-1.5 py-0.5 rounded",
                isDark ? "bg-amber-500/20 text-amber-400" : "bg-amber-100 text-amber-600"
              )}>
                F
              </span>
            )}

            {(actor.unread_count ?? 0) > 0 && (
              <span
                className={classNames(
                  "text-[10px] px-1.5 py-0.5 rounded-full font-bold",
                  isDark ? "bg-indigo-500/20 text-indigo-300" : "bg-indigo-100 text-indigo-700"
                )}
              >
                {actor.unread_count}
              </span>
            )}
          </button>
        );
      })}

      {onAddAgent && (
        <button
          onClick={onAddAgent}
          disabled={!canAddAgent}
          className={classNames(
            "ml-1 flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-xs font-medium transition-all disabled:opacity-30 whitespace-nowrap border",
            isDark 
              ? "border-white/10 text-slate-300 hover:bg-white/5" 
              : "border-black/10 text-gray-600 hover:bg-black/5"
          )}
          title={actors.length === 0 ? "Add your first agent (foreman)" : "Add agent"}
          aria-label="Add agent"
        >
          <span className="text-sm leading-none">+</span>
          <span className="hidden sm:inline">Add</span>
        </button>
      )}
    </div>
  );
}
