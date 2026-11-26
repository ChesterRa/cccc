import { useContext, useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { PendingReplySlot } from "../../components/messageBubble/PendingReplySlot";
import { useGroupStore } from "../../stores";
import type { ChatMessageData, LedgerEvent } from "../../types";
import { formatElapsed } from "../../utils/time";
import { PendingIndicatorText } from "../pendingReplies/PendingIndicator";
import { PendingSummaryContext } from "../pendingReplies/context";
import { usePendingPages } from "../pendingReplies/usePendingPages";
import { useVisiblePendingSummary } from "../pendingReplies/useVisiblePendingSummary";
import { waitingGroups, waitingIndicator } from "./chetWaiting";

function GroupRequests({
  groupId,
  title,
  active,
  onOpenGroupMessage,
}: {
  groupId: string;
  title: string;
  /** Reads only while the inbox is on screen; a closed Chet reads nothing. */
  active: boolean;
  onOpenGroupMessage?: (groupId: string, eventId: string) => void;
}) {
  const { t, i18n } = useTranslation("modals");
  const pages = usePendingPages(groupId, active);
  const open = (event: LedgerEvent) => {
    if (event.id) onOpenGroupMessage?.(groupId, event.id);
  };
  return (
    <>
      {pages.events.map((event) => {
        const seconds = event.ts
          ? Math.max(0, Math.floor((Date.now() - Date.parse(event.ts)) / 1000))
          : null;
        const text = String((event.data as ChatMessageData | undefined)?.text ?? "");
        return (
          <article
            key={event.id}
            data-chet-waiting-request={event.id}
            className="flex min-w-0 flex-col gap-1.5 border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-3 py-2.5"
          >
            <div className="flex min-w-0 items-center gap-2 text-[11px] text-[var(--color-text-tertiary)]">
              <span className="min-w-0 truncate border border-[var(--glass-border-subtle)] px-1.5 text-[var(--color-text-primary)]">
                {title}
              </span>
              {seconds !== null ? (
                <span className="ml-auto shrink-0">{formatElapsed(seconds, i18n.language)}</span>
              ) : null}
            </div>
            <div className="flex min-w-0 items-center gap-2 text-[11px] text-[var(--color-text-tertiary)]">
              <span className="min-w-0 truncate">
                {t("chet.waitingFrom", { sender: event.by || "" })}
              </span>
              {/* Leaving Chet sits apart from the answers, so a reply never misses into it. */}
              {onOpenGroupMessage ? (
                <button
                  type="button"
                  className="ml-auto shrink-0 hover:text-[var(--color-text-primary)]"
                  onClick={() => open(event)}
                >
                  {t("chet.waitingOpenInGroup")}
                </button>
              ) : null}
            </div>
            {text ? (
              <p className="line-clamp-4 min-w-0 whitespace-pre-wrap break-words text-sm text-[var(--color-text-primary)]">
                {text}
              </p>
            ) : null}
            <div className="min-w-0">
              <PendingReplySlot event={event} groupId={groupId} onUseComposer={() => open(event)} />
            </div>
          </article>
        );
      })}
      {pages.error || pages.daemonUnsupported ? (
        <p
          role="status"
          data-chet-waiting-problem={groupId}
          className="flex min-w-0 items-center gap-2 border border-dashed border-[var(--glass-border-subtle)] px-3 py-2 text-xs text-[var(--color-text-tertiary)]"
        >
          <span className="min-w-0 flex-1">
            {t(pages.daemonUnsupported ? "chet.waitingUnsupported" : "chet.waitingLoadFailed", {
              group: title,
            })}
          </span>
          {pages.daemonUnsupported ? null : (
            <button
              type="button"
              className="shrink-0 hover:text-[var(--color-text-primary)]"
              onClick={() => pages.refresh()}
            >
              {t("chet.waitingRetry")}
            </button>
          )}
        </p>
      ) : pages.loading && !pages.loaded ? (
        <p role="status" className="px-3 py-2 text-xs text-[var(--color-text-tertiary)]">
          {t("chet.waitingLoading", { group: title })}
        </p>
      ) : null}
      {pages.hasMore ? (
        <button
          type="button"
          disabled={pages.loading}
          data-chet-waiting-more={groupId}
          className="justify-self-start px-1 text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] disabled:opacity-50"
          onClick={() => pages.loadMore()}
        >
          {t("chet.waitingLoadMore", { group: title })}
        </button>
      ) : null}
    </>
  );
}

/**
 * Chet's column of what members asked the user to answer, gathered from every Group so the
 * user answers them in one place. Each answer goes back to its own Group and asker.
 */
export function ChetReplyInbox({
  active,
  onOpenGroupMessage,
}: {
  /** Whether the inbox is on screen; it reads Groups only then. */
  active: boolean;
  onOpenGroupMessage?: (groupId: string, eventId: string) => void;
}) {
  const { t } = useTranslation("modals");
  const summary = useContext(PendingSummaryContext);
  const groups = useGroupStore((state) => state.groups);
  const titles = useMemo(
    () => new Map(groups.map((group) => [String(group.group_id), group.title || group.group_id])),
    [groups],
  );
  if (!summary) return null;
  const waiting = waitingGroups(summary.summaries);
  const indicator = waitingIndicator(summary.summaries);
  return (
    <section
      aria-label={t("chet.waitingTitle")}
      data-chet-reply-inbox
      className="flex min-h-0 w-[340px] min-w-0 flex-none flex-col overflow-hidden border-r border-[var(--glass-border-subtle)]"
    >
      <header className="border-b border-[var(--glass-border-subtle)] px-4 py-3">
        <h2 className="text-sm font-semibold text-[var(--color-text-primary)]">
          {t("chet.waitingTitle")}
        </h2>
        <p className="mt-0.5 text-xs text-[var(--color-text-tertiary)]">
          {!indicator
            ? t("chet.waitingEmpty")
            : indicator.state === "fresh"
              ? t("chet.waitingSummary", { count: indicator.count ?? 0, groups: waiting.length })
              : t("chet.waitingUncertain", { value: indicator.text })}
        </p>
      </header>
      {indicator ? (
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)] content-start gap-2 overflow-y-auto overflow-x-hidden p-2">
          {waiting.map((groupId) => (
            <GroupRequests
              key={groupId}
              groupId={groupId}
              title={titles.get(groupId) ?? groupId}
              active={active}
              onOpenGroupMessage={onOpenGroupMessage}
            />
          ))}
        </div>
      ) : null}
    </section>
  );
}

/**
 * Places the inbox beside Chet's conversation where there is room for both. On a narrow
 * screen one of them fills the page, chosen from Chet's pane switch.
 */
export function ChetInboxLayout({
  inbox,
  phoneShowsInbox,
  children,
}: {
  inbox: ReactNode;
  phoneShowsInbox: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className="flex min-h-0 flex-1"
      data-chet-phone-pane={phoneShowsInbox ? "waiting" : "conversation"}
    >
      <div
        data-chet-inbox-slot
        className={`min-h-0 lg:flex lg:flex-none ${
          phoneShowsInbox ? "flex flex-1 [&>section]:w-full lg:[&>section]:w-[340px]" : "hidden"
        }`}
      >
        {inbox}
      </div>
      <div
        data-chet-conversation-slot
        className={`min-h-0 min-w-0 flex-1 flex-col lg:flex ${phoneShowsInbox ? "hidden" : "flex"}`}
      >
        {children}
      </div>
    </div>
  );
}

/** The one place the sidebar counts what is waiting for the user: beside Chet, who holds it. */
export function ChetWaitingBadge({ className }: { className?: string }) {
  const summary = useVisiblePendingSummary();
  // A count that could not be read shows as uncertain, never as zero or as an exact number.
  const indicator = summary ? waitingIndicator(summary.summaries) : null;
  if (!indicator) return null;
  return (
    <span className={className} data-chet-waiting-total={indicator.text}>
      <PendingIndicatorText indicator={indicator} />
    </span>
  );
}
