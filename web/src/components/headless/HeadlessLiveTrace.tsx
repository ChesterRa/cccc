import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import {
  buildHeadlessPreviewRenderGroups,
  buildHeadlessPreviewTimelineEntries,
  type HeadlessPreviewRenderGroup,
  type HeadlessPreviewTimelineEntry,
} from "../../pages/chat/headlessPreviewTimeline";
import type { HeadlessPreviewSession, StreamingActivity } from "../../types";
import { classNames } from "../../utils/classNames";
import { formatTime } from "../../utils/time";
import {
  formatStreamingActivityKind,
  getStructuredStreamingActivityLabel,
} from "../messageBubble/helpers";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { ClockIcon, SparklesIcon } from "../Icons";

type HeadlessLiveTraceDensity = "compact" | "expanded";

type HeadlessLiveTraceProps = {
  previewSessions?: HeadlessPreviewSession[];
  fallbackText?: string;
  fallbackActivities?: StreamingActivity[];
  fallbackUpdatedAt?: string;
  fallbackPendingEventId?: string;
  fallbackStreamId?: string;
  fallbackStreamPhase?: string;
  fallbackPhase?: string;
  emptyLabel: string;
  recentLabel?: string;
  isDark: boolean;
  density?: HeadlessLiveTraceDensity;
  className?: string;
};

type HeadlessLiveTraceLabels = {
  liveTrace: string;
  waitingForOutput: string;
  update: string;
  reply: string;
  reasoning: string;
  toolActivity: string;
  recentActivity: string;
  toolCall: string;
  toolCalls: (count: number) => string;
  live: string;
  streaming: string;
  recent: string;
};

function shouldRenderPreviewMarkdown(streamPhase: string | null | undefined): boolean {
  return String(streamPhase || "").trim().toLowerCase() === "final_answer";
}

function normalizeMetaLines(lines: Array<string | undefined>): string[] {
  const normalized: string[] = [];
  const seen = new Set<string>();
  for (const line of lines) {
    const text = String(line || "").trim();
    if (!text || seen.has(text)) continue;
    seen.add(text);
    normalized.push(text);
  }
  return normalized;
}

function normalizeInlineText(value: string | undefined): string {
  return String(value || "").replace(/\s+/g, " ").trim();
}

function truncateText(value: string, maxLength: number): string {
  const normalized = normalizeInlineText(value);
  if (!normalized || normalized.length <= maxLength) return normalized;
  return `${normalized.slice(0, Math.max(0, maxLength - 1)).trimEnd()}...`;
}

function getPathTail(path: string, depth = 2): string {
  const normalized = normalizeInlineText(path).replace(/\\/g, "/");
  if (!normalized) return "";
  const segments = normalized.split("/").filter(Boolean);
  if (segments.length <= depth) return normalized;
  return segments.slice(-depth).join("/");
}

function summarizeFilePaths(filePaths: string[], density: HeadlessLiveTraceDensity): string {
  const visibleCount = density === "expanded" ? 2 : 1;
  const visible = filePaths
    .slice(0, visibleCount)
    .map((path) => getPathTail(path, 2))
    .filter(Boolean);
  if (visible.length <= 0) return "";
  const remainingCount = filePaths.length - visible.length;
  return remainingCount > 0 ? `${visible.join(", ")} +${remainingCount}` : visible.join(", ");
}

function buildActivityNarrative(
  activity: StreamingActivity,
  density: HeadlessLiveTraceDensity
): {
  kindLabel: string;
  primaryLabel: string;
  primaryTitle: string;
  metaLines: string[];
  detailRows: Array<{ label: string; value: string }>;
  title: string;
} {
  const summary = normalizeInlineText(activity.summary);
  const detail = normalizeInlineText(activity.detail);
  const cwd = normalizeInlineText(activity.cwd);
  const query = normalizeInlineText(activity.query);
  const command = normalizeInlineText(activity.command);
  const structuredLabel = normalizeInlineText(getStructuredStreamingActivityLabel(activity));
  const filePaths = Array.isArray(activity.file_paths)
    ? activity.file_paths.map((path) => normalizeInlineText(path)).filter(Boolean)
    : [];
  const toolName = normalizeInlineText(activity.tool_name);
  const serverName = normalizeInlineText(activity.server_name);
  const status = normalizeInlineText(activity.status);
  const toolLabel = toolName ? (serverName ? `${serverName}:${toolName}` : toolName) : "";
  const primaryTitle = command
    || (filePaths.length > 0 ? filePaths.join(", ") : "")
    || structuredLabel
    || summary
    || formatStreamingActivityKind(activity.kind);
  const primaryLabel = command
    ? truncateText(command, density === "expanded" ? 92 : 76)
    : filePaths.length > 0
      ? summarizeFilePaths(filePaths, density)
      : truncateText(structuredLabel || summary || formatStreamingActivityKind(activity.kind), density === "expanded" ? 88 : 72);
  const metaLines = normalizeMetaLines([
    summary && summary !== primaryTitle ? summary : undefined,
    detail,
    query && query !== primaryTitle ? query : undefined,
    cwd ? `cwd ${cwd}` : undefined,
  ]).map((line) => truncateText(line, density === "expanded" ? 96 : 72));
  const showToolDetail = Boolean(
    toolLabel
    && toolLabel !== primaryLabel
    && toolLabel !== primaryTitle
    && toolLabel !== summary
  );
  const detailRows = normalizeMetaLines([
    detail && detail !== primaryLabel ? `detail\t${detail}` : undefined,
    showToolDetail ? `tool\t${toolLabel}` : undefined,
    command ? `cmd\t${command}` : undefined,
    query ? `query\t${query}` : undefined,
    cwd ? `cwd\t${cwd}` : undefined,
    filePaths.length > 0 ? `files\t${filePaths.map((path) => getPathTail(path, 3)).join(", ")}` : undefined,
    status ? `state\t${status}` : undefined,
  ]).map((line) => {
    const [label, ...rest] = line.split("\t");
    return {
      label,
      value: truncateText(rest.join("\t"), density === "expanded" ? 140 : 96),
    };
  }).filter((row) => row.label && row.value);
  const title = normalizeMetaLines([
    primaryTitle,
    summary && summary !== primaryTitle ? summary : undefined,
    detail,
    query && query !== primaryTitle ? query : undefined,
    cwd ? `cwd ${cwd}` : undefined,
  ]).join("\n");
  return {
    kindLabel: formatStreamingActivityKind(activity.kind),
    primaryLabel: primaryLabel || summary || formatStreamingActivityKind(activity.kind),
    primaryTitle,
    metaLines,
    detailRows,
    title,
  };
}

function getMessageTextClassName(density: HeadlessLiveTraceDensity, isDark: boolean): string {
  if (density === "expanded") {
    return classNames(
      "whitespace-pre-wrap break-words text-[13.5px] leading-[1.65]",
      isDark ? "text-slate-100" : "text-gray-900"
    );
  }
  return classNames(
    "whitespace-pre-wrap break-words text-[13px] leading-[1.55]",
    isDark ? "text-slate-100" : "text-gray-900"
  );
}

function getTracePhaseLabel(streamPhase: string, labels: HeadlessLiveTraceLabels): string {
  const normalized = String(streamPhase || "").trim().toLowerCase();
  if (normalized === "final_answer") return labels.reply;
  if (normalized === "commentary") return labels.reasoning;
  return "";
}

function getActivityBandClassName(density: HeadlessLiveTraceDensity): string {
  return density === "expanded" ? "min-w-0 flex flex-col gap-1.5" : "min-w-0 flex flex-col gap-1";
}

function getActivityRowClassName(density: HeadlessLiveTraceDensity, isDark: boolean, live: boolean): string {
  return classNames(
    "min-w-0 border backdrop-blur-sm transition-colors",
    density === "expanded"
      ? "min-h-[32px] rounded-2xl px-3 py-2"
      : "min-h-[26px] rounded-xl px-2.5 py-1.5",
    live
      ? (isDark ? "border-white/12 bg-white/[0.06] shadow-[0_12px_28px_rgba(0,0,0,0.18)]" : "border-black/10 bg-white shadow-[0_10px_24px_rgba(15,23,42,0.08)]")
      : (isDark ? "border-white/8 bg-white/[0.03]" : "border-slate-200/90 bg-slate-50/88")
  );
}

function getActivityKindBadgeClassName(density: HeadlessLiveTraceDensity, isDark: boolean, live: boolean): string {
  return classNames(
    "inline-flex shrink-0 rounded-full border font-mono font-semibold uppercase tracking-[0.14em]",
    density === "expanded" ? "px-2 py-[4px] text-[9px]" : "px-1.5 py-[2px] text-[8px]",
    live
      ? (isDark ? "border-white/12 bg-white/[0.1] text-white" : "border-black/10 bg-slate-100 text-[rgb(35,36,37)]")
      : (isDark ? "border-white/10 bg-white/[0.05] text-slate-300" : "border-slate-200 bg-white text-slate-600")
  );
}

function isErrorActivity(activity: StreamingActivity): boolean {
  return String(activity.kind || "").trim().toLowerCase() === "error";
}

function summarizeHeadline(
  group: HeadlessPreviewRenderGroup | undefined,
  density: HeadlessLiveTraceDensity,
  labels: HeadlessLiveTraceLabels
): { eyebrow: string; title: string } {
  if (!group) {
    return { eyebrow: labels.liveTrace, title: "" };
  }
  if (group.kind === "message") {
    const label = getTracePhaseLabel(group.entry.streamPhase, labels) || labels.update;
    return {
      eyebrow: label,
      title: truncateText(group.entry.text, density === "expanded" ? 120 : 84),
    };
  }
  const firstNarrative = group.entries.length === 1 && group.entries[0]
    ? buildActivityNarrative(group.entries[0].activity, density)
    : null;
  return {
    eyebrow: group.live ? labels.toolActivity : labels.recentActivity,
    title: group.entries.length > 1
      ? labels.toolCalls(group.entries.length)
      : (firstNarrative?.primaryLabel || labels.toolCall),
  };
}

function ActivityRow({
  entry,
  density,
  isDark,
}: {
  entry: Extract<HeadlessPreviewTimelineEntry, { kind: "activity" }>;
  density: HeadlessLiveTraceDensity;
  isDark: boolean;
}) {
  const narrative = buildActivityNarrative(entry.activity, density);
  const error = isErrorActivity(entry.activity);
  const trailingMeta = density === "expanded" ? narrative.metaLines[0] || "" : "";
  const showDetails = density === "expanded" && narrative.detailRows.length > 0;

  return (
    <div
      className={classNames(
        getActivityRowClassName(density, isDark, entry.live),
        error
          ? (isDark ? "border-rose-400/30 bg-rose-500/[0.08]" : "border-rose-200 bg-rose-50")
          : ""
      )}
      title={narrative.title || narrative.primaryTitle}
    >
      <div className="flex min-w-0 items-center gap-2.5">
        <span className={classNames(
          getActivityKindBadgeClassName(density, isDark, entry.live),
          error
            ? (isDark ? "border-rose-300/30 bg-rose-400/12 text-rose-100" : "border-rose-200 bg-white text-rose-700")
            : ""
        )}>
          {narrative.kindLabel}
        </span>
        <div
          className={classNames(
            "min-w-0 flex-1 truncate whitespace-nowrap font-medium",
            density === "expanded" ? "text-[12.5px] leading-5" : "text-[11.5px] leading-4",
            error ? (isDark ? "text-rose-50" : "text-rose-800") : (isDark ? "text-slate-100" : "text-slate-900")
          )}
          title={narrative.primaryTitle}
        >
          {narrative.primaryLabel}
        </div>
        {trailingMeta ? (
          <div
            className={classNames(
              "min-w-0 max-w-[34%] shrink truncate whitespace-nowrap text-[10px] leading-4",
              isDark ? "text-slate-400" : "text-slate-600"
            )}
            title={trailingMeta}
          >
            {trailingMeta}
          </div>
        ) : null}
      </div>
      {showDetails ? (
        <div className={classNames(
          "mt-2 grid gap-x-3 gap-y-1 border-t pt-2 font-mono text-[10.5px] leading-4 sm:grid-cols-[repeat(2,minmax(0,1fr))]",
          isDark ? "border-white/8 text-slate-400" : "border-slate-200/80 text-slate-600"
        )}>
          {narrative.detailRows.map((row) => (
            <div key={`${row.label}:${row.value}`} className="min-w-0">
              <span className={classNames("mr-1 uppercase tracking-[0.12em]", isDark ? "text-slate-500" : "text-slate-500")}>
                {row.label}
              </span>
              <span className="break-words">{row.value}</span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function HeadlessLiveTrace({
  previewSessions,
  fallbackText,
  fallbackActivities,
  fallbackUpdatedAt,
  fallbackPendingEventId,
  fallbackStreamId,
  fallbackStreamPhase,
  fallbackPhase,
  emptyLabel,
  recentLabel,
  isDark,
  density = "compact",
  className,
}: HeadlessLiveTraceProps) {
  const { t } = useTranslation("actors");
  const outputRef = useRef<HTMLDivElement | null>(null);
  const shouldStickToBottomRef = useRef(true);
  const labels = useMemo<HeadlessLiveTraceLabels>(() => ({
    liveTrace: t("headlessTraceLiveTrace", { defaultValue: "Live trace" }),
    waitingForOutput: t("headlessTraceWaitingForOutput", { defaultValue: "Waiting for output" }),
    update: t("headlessTraceUpdate", { defaultValue: "Update" }),
    reply: t("headlessTraceReply", { defaultValue: "Reply" }),
    reasoning: t("headlessTraceReasoning", { defaultValue: "Reasoning" }),
    toolActivity: t("headlessTraceToolActivity", { defaultValue: "Tool activity" }),
    recentActivity: t("headlessTraceRecentActivity", { defaultValue: "Recent activity" }),
    toolCall: t("headlessTraceToolCall", { defaultValue: "Tool call" }),
    toolCalls: (count: number) => t("headlessTraceToolCalls", { count, defaultValue: "{{count}} tool calls" }),
    live: t("headlessTraceLive", { defaultValue: "Live" }),
    streaming: t("headlessTraceStreaming", { defaultValue: "streaming" }),
    recent: t("headlessTraceRecent", { defaultValue: "Recent" }),
  }), [t]);
  const recentDisplayLabel = recentLabel || labels.recent;
  const timelineEntries = useMemo(
    () => buildHeadlessPreviewTimelineEntries({
      previewSessions,
      fallbackText,
      fallbackActivities,
      fallbackUpdatedAt,
      fallbackPendingEventId,
      fallbackStreamId,
      fallbackStreamPhase,
      fallbackPhase,
    }),
    [
      fallbackActivities,
      fallbackPhase,
      fallbackPendingEventId,
      fallbackStreamId,
      fallbackStreamPhase,
      fallbackText,
      fallbackUpdatedAt,
      previewSessions,
    ]
  );
  const timelineGroups = useMemo(() => buildHeadlessPreviewRenderGroups(timelineEntries), [timelineEntries]);
  const sessionCount = Array.isArray(previewSessions) && previewSessions.length > 0
    ? previewSessions.length
    : (timelineEntries.length > 0 ? 1 : 0);
  const latestTimelineGroup = timelineGroups[timelineGroups.length - 1];
  const latestSummary = summarizeHeadline(latestTimelineGroup, density, labels);
  const latestTimelineSignal = latestTimelineGroup
    ? `${latestTimelineGroup.id}:${latestTimelineGroup.ts}:${latestTimelineGroup.live ? "live" : "static"}`
    : "";
  const latestPendingEventId = String(
    latestTimelineGroup?.pendingEventId
    || fallbackPendingEventId
    || previewSessions?.[previewSessions.length - 1]?.pendingEventId
    || ""
  ).trim();

  useEffect(() => {
    shouldStickToBottomRef.current = true;
    const node = outputRef.current;
    if (!node) return;
    node.scrollTop = node.scrollHeight;
  }, [latestPendingEventId]);

  useEffect(() => {
    const node = outputRef.current;
    if (!node || !shouldStickToBottomRef.current) return;
    node.scrollTop = node.scrollHeight;
  }, [latestTimelineSignal, timelineEntries.length]);

  if (timelineGroups.length <= 0) {
    return (
      <div
        className={classNames(
          "flex min-h-[240px] items-center justify-center rounded-[26px] border px-5 py-5",
          isDark
            ? "border-white/10 bg-[linear-gradient(180deg,rgba(255,255,255,0.03),rgba(255,255,255,0.015))]"
            : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.98),rgba(248,250,252,0.94))]",
          className
        )}
      >
        <div
          className={classNames(
            "w-full max-w-md rounded-[24px] border px-6 py-6 text-center shadow-sm",
            isDark
              ? "border-white/10 bg-white/[0.04]"
              : "border-black/8 bg-white/88"
          )}
        >
          <div className="mx-auto flex h-11 w-11 items-center justify-center rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-tertiary)]">
            <SparklesIcon size={18} />
          </div>
          <div className="mt-4 text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--color-text-muted)]">
            {labels.liveTrace}
          </div>
          <div className="mt-2 text-lg font-semibold text-[var(--color-text-primary)]">{labels.waitingForOutput}</div>
          <div className={classNames("mx-auto mt-3 max-w-sm text-sm leading-6", isDark ? "text-slate-400" : "text-slate-500")}>
            {emptyLabel}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      ref={outputRef}
      onScroll={() => {
        const node = outputRef.current;
        if (!node) return;
        shouldStickToBottomRef.current = node.scrollTop + node.clientHeight >= node.scrollHeight - 24;
      }}
      className={classNames(
        "rounded-[26px] border backdrop-blur-xl",
        isDark
          ? "border-white/10 bg-[linear-gradient(180deg,rgba(15,16,20,0.94),rgba(9,10,14,0.98))] shadow-[0_28px_80px_rgba(0,0,0,0.26)]"
          : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.99),rgba(248,250,252,0.95))] shadow-[0_24px_70px_rgba(15,23,42,0.08)]",
        className
      )}
    >
      <div className={classNames(
        "sticky top-0 z-[1] border-b px-4 py-3 backdrop-blur-xl sm:px-5",
        isDark ? "border-white/8 bg-[rgba(10,11,14,0.78)]" : "border-black/6 bg-[rgba(255,255,255,0.78)]"
      )}>
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--color-text-muted)]">
              <SparklesIcon size={14} />
              {latestSummary.eyebrow}
            </div>
            <div className="mt-1 min-w-0 text-sm font-semibold leading-6 text-[var(--color-text-primary)]">
              {latestSummary.title}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {latestTimelineGroup?.live ? (
              <span className="inline-flex items-center gap-1.5 rounded-full border border-emerald-500/20 bg-emerald-500/12 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.14em] text-emerald-600 dark:text-emerald-300">
                <span className="h-1.5 w-1.5 rounded-full bg-current animate-pulse motion-reduce:animate-none" />
                {labels.live}
              </span>
            ) : null}
            <span className={classNames(
              "inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[10px] font-medium",
              isDark ? "border-white/10 bg-white/[0.04] text-slate-300" : "border-black/8 bg-black/[0.03] text-slate-600"
            )}>
              <ClockIcon size={12} />
              {latestTimelineGroup?.ts ? formatTime(latestTimelineGroup.ts) : recentDisplayLabel}
            </span>
          </div>
        </div>
      </div>

      <div className={classNames(density === "expanded" ? "space-y-4 px-4 py-4 sm:px-5 sm:py-5" : "space-y-3 px-4 py-4")}>
        {timelineGroups.map((group, index) => {
          const previousGroup = index > 0 ? timelineGroups[index - 1] : null;
          const showSessionDivider = sessionCount > 1 && previousGroup?.pendingEventId !== group.pendingEventId;
          return (
            <div key={group.id} className={classNames(density === "expanded" ? "space-y-3" : "space-y-2.5")}>
              {showSessionDivider ? (
                <div className="flex items-center gap-3 py-1">
                  <span className={classNames("h-px flex-1", isDark ? "bg-white/8" : "bg-slate-200")} />
                  <span className={classNames("shrink-0 rounded-full border px-2.5 py-1 text-[10px] font-medium uppercase tracking-[0.14em]", isDark ? "border-white/10 bg-white/[0.04] text-slate-400" : "border-black/8 bg-black/[0.03] text-slate-500")}>
                    {group.ts ? formatTime(group.ts) : recentDisplayLabel}
                  </span>
                  <span className={classNames("h-px flex-1", isDark ? "bg-white/8" : "bg-slate-200")} />
                </div>
              ) : null}

              {group.kind === "message" ? (
                <div className={classNames("grid gap-3", density === "expanded" ? "grid-cols-[12px_minmax(0,1fr)]" : "grid-cols-[10px_minmax(0,1fr)]")}>
                  <div className={classNames(density === "expanded" ? "pt-[0.75rem]" : "pt-2.5")}>
                    <span
                      className={classNames(
                        "block rounded-full",
                        density === "expanded" ? "h-3 w-3" : "h-2.5 w-2.5",
                        group.entry.streamPhase === "final_answer"
                          ? (group.entry.live ? (isDark ? "bg-white" : "bg-[rgb(35,36,37)]") : (isDark ? "bg-white/55" : "bg-[rgb(35,36,37)]/55"))
                          : (group.entry.live ? (isDark ? "bg-white" : "bg-[rgb(35,36,37)]") : (isDark ? "bg-white/30" : "bg-slate-400"))
                      )}
                    />
                  </div>
                  <div className={classNames(
                    "min-w-0 rounded-[22px] border px-4 py-3 sm:px-4.5",
                    group.entry.streamPhase === "final_answer"
                      ? (isDark ? "border-white/12 bg-white/[0.05]" : "border-black/10 bg-white shadow-[0_10px_26px_rgba(15,23,42,0.06)]")
                      : (isDark ? "border-white/10 bg-white/[0.03]" : "border-slate-200/90 bg-slate-50/88")
                  )}>
                    {getTracePhaseLabel(group.entry.streamPhase, labels) ? (
                      <div className="mb-2 flex items-center gap-2">
                        <span className={classNames(
                          "inline-flex rounded-full border px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.14em]",
                          isDark ? "border-white/10 bg-white/[0.05] text-white/85" : "border-black/8 bg-black/[0.03] text-[rgb(35,36,37)]/80"
                        )}>
                        {getTracePhaseLabel(group.entry.streamPhase, labels)}
                        </span>
                        {group.live ? (
                          <span className="inline-flex items-center gap-1 text-[10px] font-medium uppercase tracking-[0.14em] text-emerald-600 dark:text-emerald-300">
                            <span className="h-1.5 w-1.5 rounded-full bg-current animate-pulse motion-reduce:animate-none" />
                            {labels.streaming}
                          </span>
                        ) : null}
                      </div>
                    ) : null}
                    {shouldRenderPreviewMarkdown(group.entry.streamPhase) ? (
                      <LazyMarkdownRenderer
                        content={group.entry.text}
                        isDark={isDark}
                        invertText={isDark}
                        className={classNames(
                          density === "expanded"
                            ? "text-[13.5px] leading-[1.65] [&_p]:mb-2.5 [&_pre]:border [&_pre]:p-3.5 [&_.code-block-header]:rounded-t-2xl [&_.code-block-header]:border [&_.code-block-header]:px-3.5 [&_.code-block-header]:py-2.5"
                            : "text-[13px] leading-[1.58] [&_p]:mb-2 [&_pre]:border [&_pre]:p-3 [&_.code-block-header]:rounded-t-2xl [&_.code-block-header]:border [&_.code-block-header]:px-3 [&_.code-block-header]:py-2",
                          isDark
                            ? "[&_pre]:border-white/10 [&_pre]:bg-white/[0.03] [&_.code-block-header]:border-white/10 [&_.code-block-header]:bg-white/[0.04] [&_.code-block-header]:text-slate-300"
                            : "[&_pre]:border-slate-200 [&_pre]:bg-white [&_.code-block-header]:border-slate-200 [&_.code-block-header]:bg-slate-100 [&_.code-block-header]:text-slate-600"
                        )}
                        fallback={<div className={getMessageTextClassName(density, isDark)}>{group.entry.text}</div>}
                      />
                    ) : (
                      <div className={getMessageTextClassName(density, isDark)}>{group.entry.text}</div>
                    )}
                  </div>
                </div>
              ) : (
                <div className={classNames("grid gap-3", density === "expanded" ? "grid-cols-[12px_minmax(0,1fr)]" : "grid-cols-[10px_minmax(0,1fr)]")}>
                  <div className={classNames(density === "expanded" ? "pt-[0.55rem]" : "pt-[0.4rem]")}>
                    <span className={classNames(
                      "block rounded-full",
                      density === "expanded" ? "h-3 w-3" : "h-2.5 w-2.5",
                      group.live ? (isDark ? "bg-white" : "bg-[rgb(35,36,37)]") : (isDark ? "bg-white/20" : "bg-slate-400")
                    )} />
                  </div>
                  <div className={getActivityBandClassName(density)}>
                    {group.entries.map((entry) => (
                      <ActivityRow
                        key={entry.id}
                        entry={entry}
                        density={density}
                        isDark={isDark}
                      />
                    ))}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
