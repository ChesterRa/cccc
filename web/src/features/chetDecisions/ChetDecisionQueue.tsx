import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { PendingReplySlot } from "../../components/messageBubble/PendingReplySlot";
import { Button } from "../../components/ui/button";
import type { ChatMessageData } from "../../types";
import {
  filterDecisions,
  STALE_AFTER_DAYS,
  type DecisionFilter,
  type DecisionItem,
} from "./decisionQueueModel";
import { useDecisionQueue } from "./useDecisionQueue";

function DecisionCard({
  item,
  onOpen,
}: {
  item: DecisionItem;
  onOpen?: (groupId: string, eventId: string) => void;
}) {
  const { t } = useTranslation("modals");
  const data = (item.event.data ?? {}) as ChatMessageData;
  const text = String(data.text || "").trim();
  const [expanded, setExpanded] = useState(false);
  const long = text.length > 420 || text.split("\n").length > 8;
  const eventId = String(item.event.id || "");
  const open = onOpen ? () => onOpen(item.groupId, eventId) : undefined;
  return (
    <article
      className="relative rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4"
      data-chet-decision={eventId}
      data-chet-decision-stale={item.stale || undefined}
      tabIndex={-1}
    >
      <header className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-[var(--color-text-tertiary)]">
        <span
          className="max-w-[16rem] truncate rounded-md bg-[var(--glass-tab-bg)] px-1.5 py-0.5 font-medium text-[var(--color-text-secondary)]"
          title={item.groupTitle}
        >
          {item.groupTitle}
        </span>
        <span className="font-semibold text-[var(--color-text-primary)]">{item.event.by}</span>
        {item.ageDays === null ? null : (
          <span className="tabular-nums">
            {item.ageDays === 0
              ? t("chetDecisions.today")
              : t("chetDecisions.daysAgo", { count: item.ageDays })}
          </span>
        )}
        {item.stale ? (
          <span
            className="rounded-md bg-amber-400/15 px-1.5 py-0.5 text-amber-700 dark:text-amber-300"
            data-chet-decision-stale-tag
          >
            {t("chetDecisions.maybeExpired")}
          </span>
        ) : null}
        {open ? (
          <button
            type="button"
            className="ml-auto rounded-sm underline-offset-2 hover:text-[var(--color-text-primary)] hover:underline"
            onClick={open}
          >
            {t("chetDecisions.openInGroup")}
          </button>
        ) : null}
      </header>
      {text ? (
        <div
          className={`mt-2 whitespace-pre-wrap break-words text-sm leading-6 text-[var(--color-text-primary)] ${
            long && !expanded ? "line-clamp-6" : ""
          }`}
          data-chet-decision-text
        >
          {text}
        </div>
      ) : null}
      {long ? (
        <button
          type="button"
          className="mt-1 text-xs text-[var(--color-text-tertiary)] underline-offset-2 hover:underline"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {t(expanded ? "chetDecisions.showLess" : "chetDecisions.showMore")}
        </button>
      ) : null}
      <div className="mt-3">
        {/* The same card and send path as in the Group's conversation. */}
        <PendingReplySlot
          event={item.event}
          groupId={item.groupId}
          onUseComposer={open ?? (() => {})}
        />
      </div>
    </article>
  );
}

/**
 * Every Group's open questions in one place, answered where they are shown. J and K move
 * between questions; a question older than the stale line is marked, never closed.
 */
export function ChetDecisionQueue({
  onOpenGroupMessage,
  onClose,
}: {
  onOpenGroupMessage?: (groupId: string, eventId: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("modals");
  const queue = useDecisionQueue(true);
  const [filter, setFilter] = useState<DecisionFilter>("all");
  const shown = useMemo(() => filterDecisions(queue.items, filter), [filter, queue.items]);
  const staleCount = useMemo(() => queue.items.filter((item) => item.stale).length, [queue.items]);
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key !== "j" && key !== "k") return;
      const target = event.target instanceof Element ? event.target : null;
      // Typing a "j" into an answer is typing, not moving.
      if (target?.closest('input:not([type="radio"]), textarea, select, [contenteditable="true"]'))
        return;
      const cards = [
        ...(list.current?.querySelectorAll<HTMLElement>("[data-chet-decision]") ?? []),
      ];
      if (!cards.length) return;
      const current = cards.findIndex((card) => card.contains(document.activeElement));
      const next = key === "j" ? Math.min(cards.length - 1, current + 1) : Math.max(0, current - 1);
      const card = cards[current < 0 ? 0 : next]!;
      event.preventDefault();
      (card.querySelector<HTMLElement>('input[type="radio"]:not(:disabled)') ?? card).focus();
      card.scrollIntoView({ block: "nearest" });
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  const filters: [DecisionFilter, string, number][] = [
    ["all", t("chetDecisions.filterAll"), queue.items.length],
    ["fresh", t("chetDecisions.filterFresh"), queue.items.length - staleCount],
    ["stale", t("chetDecisions.filterStale"), staleCount],
  ];
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-chet-decisions>
      <div className="flex flex-none flex-wrap items-center gap-2 border-b border-[var(--glass-border-subtle)] px-4 py-2 sm:px-6">
        <div
          className="mx-auto flex w-full max-w-3xl flex-wrap items-center gap-2"
          role="group"
          aria-label={t("chetDecisions.title")}
        >
          <span className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t("chetDecisions.title")}
          </span>
          {filters.map(([value, label, count]) => (
            <button
              key={value}
              type="button"
              aria-pressed={filter === value}
              onClick={() => setFilter(value)}
              className={`rounded-md border px-2 py-1 text-xs ${
                filter === value
                  ? "border-[var(--color-border-secondary)] bg-[var(--glass-tab-bg)] font-medium text-[var(--color-text-primary)]"
                  : "border-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)]"
              }`}
              data-chet-decision-filter={value}
            >
              {label} · {count}
            </button>
          ))}
          <span className="ml-auto flex items-center gap-1">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-lcd-key
              disabled={queue.loading}
              onClick={queue.refresh}
            >
              {t("chetDecisions.refresh")}
            </Button>
            <Button type="button" variant="outline" size="sm" data-lcd-key onClick={onClose}>
              {t("chetDecisions.back")}
            </Button>
          </span>
        </div>
      </div>
      {/* Positioned, so the screen-reader-only labels inside its cards are placed within it.
          Without a positioned ancestor here they resolve against Chet's outer body, whose
          scroll area then runs the whole length of the queue and can scroll the composer
          away, leaving a blank page. */}
      <div
        ref={list}
        className="min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-6"
        style={{ position: "relative" }}
        data-chet-decision-list
      >
        <div className="mx-auto flex max-w-3xl flex-col gap-3">
          {queue.failed.length || queue.partial.length ? (
            <p className="text-xs text-amber-700 dark:text-amber-300" role="status">
              {[
                queue.failed.length
                  ? t("chetDecisions.someFailed", { count: queue.failed.length })
                  : "",
                queue.partial.length
                  ? t("chetDecisions.somePartial", { count: queue.partial.length })
                  : "",
              ]
                .filter(Boolean)
                .join(" ")}
            </p>
          ) : null}
          {filter === "stale" && shown.length ? (
            <p className="text-xs text-[var(--color-text-tertiary)]">
              {t("chetDecisions.staleHint", { days: STALE_AFTER_DAYS })}
            </p>
          ) : null}
          {shown.map((item) => (
            <DecisionCard
              key={`${item.groupId}:${item.event.id}`}
              item={item}
              onOpen={onOpenGroupMessage}
            />
          ))}
          {shown.length === 0 ? (
            <p className="py-10 text-center text-sm text-[var(--color-text-muted)]" role="status">
              {!queue.available
                ? t("chetDecisions.unavailable")
                : queue.loading
                  ? t("chetDecisions.loading")
                  : t("chetDecisions.empty")}
            </p>
          ) : null}
        </div>
      </div>
    </div>
  );
}
