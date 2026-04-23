import { useEffect, useMemo, useRef, useState } from "react";

import type { HeadlessStreamEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import { formatTime } from "../../utils/time";
import { buildHeadlessRawTraceEntries } from "../../utils/headlessRawTimeline";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { AlertIcon, ChevronDownIcon, ClockIcon, SparklesIcon } from "../Icons";

type HeadlessRawTraceProps = {
  events: HeadlessStreamEvent[];
  emptyLabel: string;
  isDark: boolean;
  className?: string;
};

function isMarkdownPhase(streamPhase: string): boolean {
  return String(streamPhase || "").trim().toLowerCase() === "final_answer";
}

function badgeClassName(tone: "neutral" | "info" | "success" | "warning" | "error", isDark: boolean): string {
  if (tone === "error") return isDark ? "border-transparent bg-rose-400/12 text-rose-100" : "border-transparent bg-rose-50 text-rose-700";
  if (tone === "warning") return isDark ? "border-transparent bg-amber-400/12 text-amber-100" : "border-transparent bg-amber-50 text-amber-700";
  if (tone === "success") return isDark ? "border-transparent bg-emerald-400/12 text-emerald-100" : "border-transparent bg-emerald-50 text-emerald-700";
  if (tone === "info") return isDark ? "border-transparent bg-sky-400/12 text-sky-100" : "border-transparent bg-sky-50 text-sky-700";
  return isDark ? "border-transparent bg-white/[0.07] text-slate-200" : "border-transparent bg-slate-100 text-slate-700";
}

function liveStatusClassName(isDark: boolean): string {
  return isDark ? "border-white/10 bg-white/[0.06] text-slate-100" : "border-slate-200 bg-white text-slate-600";
}

function messageCardClassName(live: boolean, isDark: boolean): string {
  if (live) {
    return isDark
      ? "border-white/10 bg-white/[0.05] shadow-[0_18px_40px_-34px_rgba(15,23,42,0.8)]"
      : "border-slate-200 bg-white shadow-[0_16px_36px_-34px_rgba(15,23,42,0.22)]";
  }
  return isDark ? "border-white/10 bg-white/[0.04]" : "border-slate-200 bg-slate-50/85";
}

function eventCardClassName(
  tone: "neutral" | "info" | "success" | "warning" | "error",
  live: boolean,
  isDark: boolean
): string {
  if (tone === "error") {
    return isDark
      ? "border-rose-400/20 bg-rose-500/[0.07] shadow-[0_18px_42px_-36px_rgba(244,63,94,0.55)]"
      : "border-rose-200 bg-rose-50/70 shadow-[0_18px_42px_-36px_rgba(244,63,94,0.18)]";
  }
  if (live) {
    return isDark
      ? "border-white/10 bg-white/[0.05] shadow-[0_18px_42px_-36px_rgba(56,189,248,0.18)]"
      : "border-slate-200 bg-white shadow-[0_18px_42px_-36px_rgba(15,23,42,0.12)]";
  }
  return isDark ? "border-white/10 bg-white/[0.03]" : "border-slate-200 bg-white";
}

function messageStatusLabel(streamPhase: string, live: boolean): string {
  const normalized = String(streamPhase || "").trim().toLowerCase();
  if (!live) return "done";
  if (normalized === "final_answer") return "replying";
  return "thinking";
}

export function HeadlessRawTrace({
  events,
  emptyLabel,
  isDark,
  className,
}: HeadlessRawTraceProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const shouldStickToBottomRef = useRef(true);
  const entries = useMemo(() => buildHeadlessRawTraceEntries(events), [events]);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    const node = scrollRef.current;
    shouldStickToBottomRef.current = true;
    if (!node) return;
    node.scrollTop = node.scrollHeight;
  }, [events[0]?.id, events.length > 0 ? events[events.length - 1]?.id : ""]);

  useEffect(() => {
    const node = scrollRef.current;
    if (!node || !shouldStickToBottomRef.current) return;
    node.scrollTop = node.scrollHeight;
  }, [entries]);

  useEffect(() => {
    setExpandedIds((current) => {
      const next = new Set<string>();
      for (const id of current) {
        if (entries.some((entry) => entry.id === id && entry.kind === "event")) next.add(id);
      }
      for (const entry of entries) {
        if (entry.kind === "event" && (entry.live || entry.tone === "error")) next.add(entry.id);
      }
      return next;
    });
  }, [entries]);

  if (entries.length <= 0) {
    return (
      <div
        className={classNames(
          "flex h-full min-h-[420px] items-center justify-center rounded-[28px] border border-dashed text-sm",
          isDark ? "border-white/10 text-slate-400" : "border-slate-200 text-slate-500",
          className
        )}
      >
        {emptyLabel}
      </div>
    );
  }

  return (
    <div
      ref={scrollRef}
      onScroll={() => {
        const node = scrollRef.current;
        if (!node) return;
        shouldStickToBottomRef.current = node.scrollTop + node.clientHeight >= node.scrollHeight - 24;
      }}
      className={classNames(
        "h-full min-h-[420px] overflow-y-auto rounded-[28px] border px-4 py-4 sm:px-5",
        isDark ? "border-white/10 bg-slate-950/55" : "border-slate-200 bg-white",
        className
      )}
    >
      <div className="flex flex-col gap-3">
        {entries.map((entry) => {
          if (entry.kind === "message") {
            return (
              <section
                key={entry.id}
                className={classNames(
                  "rounded-[24px] border px-4 py-3 shadow-[0_14px_34px_-30px_rgba(15,23,42,0.35)] transition-all",
                  messageCardClassName(entry.live, isDark)
                )}
              >
                <div className="mb-3 flex items-center gap-2 text-[11px]">
                  <div className={classNames(
                    "text-[13px] font-medium leading-6",
                    isDark ? "text-slate-100" : "text-slate-900"
                  )}>
                    {messageStatusLabel(entry.streamPhase, entry.live)}
                  </div>
                  <span className={classNames(
                    "inline-flex items-center gap-1 rounded-full border px-2 py-1 text-[10px] font-medium tracking-[0.08em]",
                    liveStatusClassName(isDark)
                  )}>
                    {entry.live ? <SparklesIcon className="h-3 w-3" /> : null}
                    {entry.live ? "live" : "done"}
                  </span>
                  {entry.ts ? (
                    <span className="ml-auto inline-flex items-center gap-1 text-[var(--color-text-tertiary)]">
                      <ClockIcon className="h-3.5 w-3.5" />
                      {formatTime(entry.ts)}
                    </span>
                  ) : null}
                </div>
                {isMarkdownPhase(entry.streamPhase) ? (
                  <LazyMarkdownRenderer
                    content={entry.text}
                    isDark={isDark}
                    invertText={false}
                    className="max-w-full break-words text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
                  />
                ) : (
                  <div className={classNames(
                    "whitespace-pre-wrap break-words text-[13px] leading-[1.7]",
                    isDark ? "text-slate-100" : "text-slate-900"
                  )}>
                    {entry.text}
                  </div>
                )}
              </section>
            );
          }

          const isExpanded = expandedIds.has(entry.id);
          const hasDetails = entry.detailLines.length > 0;
          const collapsedPreview = entry.detailLines[0] || "";

          return (
            <section
              key={entry.id}
              className={classNames(
                "overflow-hidden rounded-[22px] border transition-all",
                eventCardClassName(entry.tone, entry.live, isDark)
              )}
            >
              <button
                type="button"
                onClick={() => {
                  if (!hasDetails) return;
                  setExpandedIds((current) => {
                    const next = new Set(current);
                    if (next.has(entry.id)) next.delete(entry.id);
                    else next.add(entry.id);
                    return next;
                  });
                }}
                className={classNames(
                  "flex w-full items-start gap-3 px-4 py-3 text-left",
                  hasDetails ? "cursor-pointer" : "cursor-default"
                )}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-start gap-3">
                    <div className="min-w-0 flex-1">
                      <div className={classNames(
                        "text-[13px] font-medium leading-6",
                        entry.tone === "error"
                          ? (isDark ? "text-rose-100" : "text-rose-800")
                          : isDark ? "text-slate-100" : "text-slate-900"
                      )}>
                        {entry.title}
                      </div>
                      {!isExpanded && collapsedPreview ? (
                        <div className={classNames(
                          "mt-0.5 truncate text-[12px] leading-5",
                          entry.tone === "error"
                            ? (isDark ? "text-rose-200/80" : "text-rose-700")
                            : isDark ? "text-slate-400" : "text-slate-500"
                        )}>
                          {collapsedPreview}
                        </div>
                      ) : null}
                    </div>
                    <div className="flex shrink-0 items-center gap-2 text-[11px] text-[var(--color-text-tertiary)]">
                      <span className={classNames(
                        "inline-flex items-center gap-1 rounded-full border px-2 py-1 text-[10px] font-medium tracking-[0.08em]",
                        badgeClassName(entry.tone, isDark)
                      )}>
                        <span className={classNames(
                          "h-1.5 w-1.5 rounded-full",
                          entry.tone === "error"
                            ? "bg-rose-500"
                            : entry.live
                              ? "bg-sky-500"
                              : entry.tone === "success"
                                ? "bg-emerald-500"
                                : entry.tone === "warning"
                                  ? "bg-amber-500"
                                  : isDark ? "bg-slate-400" : "bg-slate-500"
                        )} />
                        {entry.badge.toLowerCase()}
                      </span>
                      {entry.live ? (
                        <span className={classNames(
                          "inline-flex items-center rounded-full border px-2 py-1 text-[10px] font-medium tracking-[0.08em]",
                          liveStatusClassName(isDark)
                        )}>
                          running
                        </span>
                      ) : null}
                      {entry.tone === "error" ? <AlertIcon className="h-4 w-4" /> : null}
                      {entry.ts ? <span>{formatTime(entry.ts)}</span> : null}
                      {hasDetails ? (
                        <ChevronDownIcon
                          className={classNames("h-4 w-4 transition-transform", isExpanded ? "rotate-180" : "")}
                        />
                      ) : null}
                    </div>
                  </div>
                  {isExpanded && hasDetails ? (
                    <div className={classNames(
                      "mt-2 whitespace-pre-wrap break-words rounded-2xl border px-3 py-2 text-[12px] leading-6",
                      entry.tone === "error"
                        ? (isDark ? "border-rose-400/20 bg-rose-400/[0.05] text-rose-200/90" : "border-rose-200 bg-white/70 text-rose-700")
                        : isDark ? "border-white/8 bg-black/20 text-slate-300" : "border-slate-200 bg-slate-50/80 text-slate-600"
                    )}>
                      {entry.detailLines.join("\n")}
                    </div>
                  ) : null}
                </div>
              </button>
            </section>
          );
        })}
      </div>
    </div>
  );
}
