import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { PendingIndicatorText } from "../../features/pendingReplies/PendingIndicator";
import type { PendingIndicator } from "../../features/pendingReplies/indicator";
import type { ChatFilter } from "../../stores/useUIStore";
import { classNames } from "../../utils/classNames";

const FADE_PX = 24;
// A few pixels past an edge are a tab's own padding, not a tab out of view.
const SLACK_PX = 4;

export function ChatFilterTabs({
  filter,
  onChange,
  waiting,
  pinned,
  isSmallScreen,
}: {
  filter: ChatFilter;
  onChange: (filter: ChatFilter) => void;
  waiting: PendingIndicator | null;
  /** Pinned is offered with its count, which is null until it is known; null hides the tab. */
  pinned: { count: number | null } | null;
  isSmallScreen: boolean;
}) {
  const { t } = useTranslation("chat");
  const tabsRef = useRef<HTMLDivElement>(null);
  // Pinned follows All, so on a phone it stays in view, or at least starts to, before the
  // strip has to scroll.
  const options: Array<[ChatFilter, string]> = [["all", t("filterAll")]];
  if (pinned) options.push(["pinned", t("filterPinned")]);
  options.push(["user", t("filterUser")]);
  if (waiting) options.unshift(["pending_user", t("filterWaitingForYou")]);
  const shown = options.map(([key]) => key).join(" ");
  // On a narrow screen the tabs scroll sideways without a scrollbar; an edge with more tabs
  // past it fades out, so a tab out of view, such as Pinned, can still be found.
  const [more, setMore] = useState({ before: false, after: false });
  useLayoutEffect(() => {
    const tabs = tabsRef.current;
    if (!tabs) return;
    const measure = () => {
      const before = tabs.scrollLeft > SLACK_PX;
      const after = tabs.scrollWidth - tabs.clientWidth - tabs.scrollLeft > SLACK_PX;
      setMore((current) =>
        current.before === before && current.after === after ? current : { before, after },
      );
    };
    measure();
    tabs.addEventListener("scroll", measure, { passive: true });
    const observer = new ResizeObserver(measure);
    observer.observe(tabs);
    for (const tab of Array.from(tabs.children)) observer.observe(tab);
    return () => {
      tabs.removeEventListener("scroll", measure);
      observer.disconnect();
    };
  }, [shown]);
  useLayoutEffect(() => {
    if (!isSmallScreen) return;
    const tabs = tabsRef.current;
    const active = tabs?.querySelector<HTMLButtonElement>('[aria-pressed="true"]');
    if (!tabs || !active) return;
    const reveal = () => bringClear(tabs, active);
    reveal();
    const observer = new ResizeObserver(reveal);
    observer.observe(tabs);
    observer.observe(active);
    return () => observer.disconnect();
  }, [filter, isSmallScreen, waiting?.text]);
  return (
    <div
      ref={tabsRef}
      className="flex h-[44px] md:h-9 min-w-0 flex-1 md:flex-initial items-stretch gap-3 overflow-x-auto scrollbar-hide"
      style={fadeMask(more)}
      data-chat-filters
      data-more-before={more.before || undefined}
      data-more-after={more.after || undefined}
    >
      {options.map(([key, label]) => (
        <button
          key={key}
          type="button"
          className={classNames(
            "inline-flex shrink-0 items-center gap-1.5 px-0.5 text-[13px] leading-5 whitespace-nowrap transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--color-text-secondary)]",
            filter === key
              ? "font-semibold text-[var(--color-text-primary)] shadow-[inset_0_-2px_0_var(--color-text-primary)]"
              : "font-medium text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]",
          )}
          onClick={() => onChange(key)}
          onFocus={(event) => {
            // Only keyboard focus. A mouse focuses the tab on press, and scrolling the strip
            // then would move the tab from under the pointer before the click completes;
            // the selected tab is brought clear after the click instead.
            if (tabsRef.current && event.currentTarget.matches(":focus-visible"))
              bringClear(tabsRef.current, event.currentTarget);
          }}
          aria-pressed={filter === key}
        >
          {key === "pending_user" && isSmallScreen ? (
            // A phone shows the short name, which leaves room for the tabs after it.
            <>
              <span aria-hidden="true">{t("filterWaitingForYouShort")}</span>
              <span className="sr-only">{label}</span>
            </>
          ) : (
            label
          )}
          {key === "pending_user" && waiting ? <PendingIndicatorText indicator={waiting} /> : null}
          {key === "pinned" && pinned?.count ? (
            <span className="tabular-nums text-[var(--color-text-secondary)]" data-pinned-count>
              {pinned.count}
            </span>
          ) : null}
        </button>
      ))}
    </div>
  );
}

// The selected tab, or one reached with the keyboard, is brought clear of the fades.
function bringClear(tabs: HTMLElement, tab: HTMLElement) {
  if (!tabs.clientWidth) return;
  const strip = tabs.getBoundingClientRect();
  const box = tab.getBoundingClientRect();
  if (box.left < strip.left + FADE_PX && tabs.scrollLeft > 0)
    tabs.scrollLeft += box.left - strip.left - FADE_PX;
  else if (box.right > strip.right - FADE_PX) tabs.scrollLeft += box.right - strip.right + FADE_PX;
}

function fadeMask(more: { before: boolean; after: boolean }) {
  if (!more.before && !more.after) return undefined;
  const start = more.before ? `transparent, #000 ${FADE_PX}px` : "#000";
  const end = more.after ? `#000 calc(100% - ${FADE_PX}px), transparent` : "#000";
  const mask = `linear-gradient(to right, ${start}, ${end})`;
  return { maskImage: mask, WebkitMaskImage: mask };
}
