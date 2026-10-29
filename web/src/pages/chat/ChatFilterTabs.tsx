import { useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { PendingIndicatorText } from "../../features/pendingReplies/PendingIndicator";
import type { PendingIndicator } from "../../features/pendingReplies/indicator";
import type { ChatFilter } from "../../stores/useUIStore";
import { classNames } from "../../utils/classNames";

export function ChatFilterTabs({
  filter,
  onChange,
  waiting,
  isSmallScreen,
}: {
  filter: ChatFilter;
  onChange: (filter: ChatFilter) => void;
  waiting: PendingIndicator | null;
  isSmallScreen: boolean;
}) {
  const { t } = useTranslation("chat");
  const tabsRef = useRef<HTMLDivElement>(null);
  const options: Array<[ChatFilter, string]> = [
    ["all", t("filterAll")],
    ["user", t("filterUser")],
    ["mail", t("filterMail")],
    ["request_reply", t("filterNeedReply")],
  ];
  if (waiting) options.unshift(["pending_user", t("filterWaitingForYou")]);
  useLayoutEffect(() => {
    if (!isSmallScreen) return;
    const tabs = tabsRef.current;
    const active = tabs?.querySelector<HTMLButtonElement>('[aria-pressed="true"]');
    if (!tabs || !active) return;
    const reveal = () => {
      if (!tabs.clientWidth) return;
      const strip = tabs.getBoundingClientRect();
      const selected = active.getBoundingClientRect();
      if (selected.left < strip.left) tabs.scrollLeft += selected.left - strip.left;
      else if (selected.right > strip.right) tabs.scrollLeft += selected.right - strip.right;
    };
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
          aria-pressed={filter === key}
        >
          {label}
          {key === "pending_user" && waiting ? <PendingIndicatorText indicator={waiting} /> : null}
        </button>
      ))}
    </div>
  );
}
