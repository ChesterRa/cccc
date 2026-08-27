import { useTranslation } from "react-i18next";

import { MessageSquareTextIcon } from "../Icons";

export function VirtualMessageListEmptyState({
  isLoadingHistory,
  hasMoreHistory,
  isFilteredEmpty,
  onLoadMore,
}: {
  isLoadingHistory: boolean;
  hasMoreHistory: boolean;
  isFilteredEmpty: boolean;
  onLoadMore?: () => void;
}) {
  const { t } = useTranslation("chat");

  if (isLoadingHistory) {
    return (
      <div className="flex h-full flex-col items-center justify-center pb-20 text-center">
        <div className="glass-panel flex items-center gap-2 rounded-full px-3 py-1.5 text-[var(--color-text-secondary)] shadow-md">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
          <span className="text-xs">{t("loadingHistory", { defaultValue: "Loading..." })}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col items-center justify-center pb-20 text-center">
      <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-muted)]">
        <MessageSquareTextIcon size={28} />
      </div>
      <p className="text-sm font-medium text-[var(--color-text-secondary)]">
        {t(isFilteredEmpty ? "noResults" : "emptyStateTitle")}
      </p>
      {!isFilteredEmpty ? (
        <>
          <p className="mt-1 text-xs text-[var(--color-text-tertiary)]">
            {t("emptyStateSubtitle")}
          </p>
          <div className="mt-4 w-full max-w-sm space-y-2 text-left text-xs text-[var(--color-text-tertiary)]">
            {[
              [t("emptyStateQuickNoteTitle"), t("emptyStateQuickNoteBody")],
              [t("emptyStateAskForemanTitle"), t("emptyStateAskForemanBody")],
              [t("emptyStateDurableTitle"), t("emptyStateDurableBody")],
            ].map(([title, body]) => (
              <div
                key={title}
                className="flex gap-2 border-t border-[var(--glass-border-subtle)] pt-2"
              >
                <span className="text-[var(--color-text-secondary)]">{title}</span>
                <span>{body}</span>
              </div>
            ))}
          </div>
        </>
      ) : null}
      {hasMoreHistory && onLoadMore ? (
        <button
          type="button"
          className="mt-4 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-4 py-2 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45"
          onClick={onLoadMore}
        >
          {t("loadOlderResults")}
        </button>
      ) : null}
    </div>
  );
}
