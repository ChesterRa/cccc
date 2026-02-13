import { useRef, useEffect, useState, useCallback, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation("layout");
  const rootRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const tabsRef = useRef<HTMLDivElement>(null);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const activeTabRef = useRef<HTMLButtonElement>(null);
  const [isOverflowing, setIsOverflowing] = useState(false);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  // Check if content overflows
  const checkOverflow = useCallback(() => {
    const rootEl = rootRef.current;
    const scrollEl = scrollRef.current;
    const tabsEl = tabsRef.current;
    if (!rootEl || !scrollEl || !tabsEl) return;

    if (!onAddAgent) {
      setIsOverflowing(prev => prev === false ? prev : false);
      return;
    }

    const rootWidth = rootEl.clientWidth;
    const tabsWidth = tabsEl.scrollWidth;
    const addButtonWidth = addButtonRef.current?.offsetWidth ?? 0;

    const scrollStyle = window.getComputedStyle(scrollEl);
    const paddingX = parseFloat(scrollStyle.paddingLeft || "0") + parseFloat(scrollStyle.paddingRight || "0");
    const gap = parseFloat(scrollStyle.columnGap || scrollStyle.gap || "0");

    const required = tabsWidth + (addButtonWidth > 0 ? addButtonWidth + gap : 0);
    const available = Math.max(0, rootWidth - paddingX);
    const newValue = required > available;
    // Only update state if value actually changed to avoid unnecessary re-renders
    setIsOverflowing(prev => prev === newValue ? prev : newValue);

    // Check scroll fade edges
    const tol = 2;
    const sl = scrollEl.scrollLeft;
    const sw = scrollEl.scrollWidth;
    const cw = scrollEl.clientWidth;
    setCanScrollLeft(sl > tol);
    setCanScrollRight(sl + cw < sw - tol);
  }, [onAddAgent]);

  // Check overflow on mount, resize, and when content changes
  useEffect(() => {
    const rootEl = rootRef.current;
    if (!rootEl) return;

    // Use requestAnimationFrame to avoid synchronous setState in effect body
    const rafId = requestAnimationFrame(checkOverflow);

    // Handle resize via ResizeObserver or fallback to window resize
    if (typeof ResizeObserver !== "undefined") {
      const ro = new ResizeObserver(() => checkOverflow());
      ro.observe(rootEl);
      return () => {
        cancelAnimationFrame(rafId);
        ro.disconnect();
      };
    } else {
      window.addEventListener("resize", checkOverflow);
      return () => {
        cancelAnimationFrame(rafId);
        window.removeEventListener("resize", checkOverflow);
      };
    }
  }, [checkOverflow, actors, unreadChatCount]);

  // Auto-scroll active tab into view
  useEffect(() => {
    if (!activeTabRef.current || !scrollRef.current) return;
    const container = scrollRef.current;
    const tab = activeTabRef.current;
    requestAnimationFrame(() => {
      const containerRect = container.getBoundingClientRect();
      const tabRect = tab.getBoundingClientRect();

      if (tabRect.left < containerRect.left) {
        container.scrollLeft -= containerRect.left - tabRect.left + 16;
      } else if (tabRect.right > containerRect.right) {
        container.scrollLeft += tabRect.right - containerRect.right + 16;
      }
    });
  }, [activeTab]);

  const addButton = onAddAgent && (
    <button
      ref={addButtonRef}
      onClick={onAddAgent}
      disabled={!canAddAgent}
      className={classNames(
        "flex-shrink-0 flex items-center justify-center w-9 h-9 rounded-lg text-sm font-medium transition-all disabled:opacity-30 focus:outline-none border",
        isDark
          ? "border-white/10 text-slate-300 hover:bg-white/5 hover:text-white"
          : "border-black/10 text-gray-600 hover:bg-black/5 hover:text-gray-900"
      )}
      title={actors.length === 0 ? t("addFirstAgent") : t("addAgent")}
      aria-label={t("addAgent")}
    >
      +
    </button>
  );

  return (
    <div
      ref={rootRef}
      className={classNames(
        "flex items-center border-b sticky top-0 z-10 backdrop-blur-md",
        isDark ? "border-white/5 bg-slate-900/70" : "border-black/5 bg-white/70"
      )}
      role="tablist"
      aria-label={t("navigationTabs")}
    >
      {/* Scrollable tabs area */}
      <div
        ref={scrollRef}
        className="flex-1 flex items-center gap-1.5 px-3 py-1.5 overflow-x-auto min-w-0 scrollbar-hide"
        style={{
          WebkitOverflowScrolling: "touch",
          ...(canScrollLeft && canScrollRight
            ? { WebkitMaskImage: "linear-gradient(to right, transparent, black 20px, black calc(100% - 20px), transparent)", maskImage: "linear-gradient(to right, transparent, black 20px, black calc(100% - 20px), transparent)" } as CSSProperties
            : canScrollLeft
              ? { WebkitMaskImage: "linear-gradient(to right, transparent, black 20px)", maskImage: "linear-gradient(to right, transparent, black 20px)" } as CSSProperties
              : canScrollRight
                ? { WebkitMaskImage: "linear-gradient(to left, transparent, black 20px)", maskImage: "linear-gradient(to left, transparent, black 20px)" } as CSSProperties
                : {}),
        }}
        onScroll={checkOverflow}
      >
        <div ref={tabsRef} className="flex items-center gap-1.5 min-w-0">
          {/* Chat Tab */}
          <button
            ref={activeTab === "chat" ? activeTabRef : null}
            onClick={() => onTabChange("chat")}
            className={classNames(
              "relative flex items-center gap-2 px-3 py-2 text-sm font-medium whitespace-nowrap transition-all rounded-lg flex-shrink-0 focus:outline-none",
              activeTab === "chat"
                ? isDark ? "bg-white/10 text-white" : "bg-black/5 text-gray-900"
                : isDark ? "text-slate-400 hover:text-slate-200 hover:bg-white/5" : "text-gray-500 hover:text-gray-700 hover:bg-black/5"
            )}
            role="tab"
            aria-selected={activeTab === "chat"}
          >
            <span>{t("chat")}</span>
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
          {actors.length > 0 && (
            <div className={`w-px h-4 flex-shrink-0 ${isDark ? "bg-white/10" : "bg-black/8"}`} />
          )}

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
                  "relative flex items-center gap-2 px-3 py-2 text-sm font-medium whitespace-nowrap transition-all rounded-lg flex-shrink-0 focus:outline-none",
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
                    "w-2 h-2 rounded-full transition-all flex-shrink-0",
                    isRunning
                      ? "bg-emerald-500"
                      : isDark ? "bg-slate-600" : "bg-gray-300"
                  )}
                />

                <span>{actor.title || actor.id}</span>

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
        </div>

        {/* Add button inside scroll area when not overflowing */}
        {!isOverflowing && addButton}
      </div>

      {/* Fixed Add Button when overflowing */}
      {isOverflowing && onAddAgent && (
        <div className={classNames(
          "flex-shrink-0 px-2 py-1.5 border-l",
          isDark ? "border-white/5" : "border-black/5"
        )}>
          {addButton}
        </div>
      )}
    </div>
  );
}
