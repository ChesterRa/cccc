import { useMemo } from "react";
import type {
  HeadlessPreviewSession,
  HeadlessStreamEvent,
  LedgerEvent,
  StreamingActivity,
} from "../../types";
import { classNames } from "../../utils/classNames";
import { HeadlessLiveTrace } from "./HeadlessLiveTrace";
import { HeadlessRawTrace } from "./HeadlessRawTrace";
import { projectUnboundTrace } from "./unboundTraceProjection";

type HeadlessRuntimePanelProps = {
  actorId: string;
  groupId: string;
  streamingEvents: LedgerEvent[];
  previewSessions: HeadlessPreviewSession[];
  fallbackText: string;
  fallbackActivities: StreamingActivity[];
  rawEvents: HeadlessStreamEvent[];
  emptyLabel: string;
  isDark: boolean;
  compact?: boolean;
};

export function HeadlessRuntimePanel({
  actorId,
  groupId,
  streamingEvents,
  previewSessions,
  fallbackText,
  fallbackActivities,
  rawEvents,
  emptyLabel,
  isDark,
  compact = false,
}: HeadlessRuntimePanelProps) {
  const minHeight = compact ? "min-h-0" : "min-h-[420px]";
  const unbound = useMemo(
    () => projectUnboundTrace(groupId, actorId, streamingEvents, previewSessions),
    [groupId, actorId, streamingEvents, previewSessions],
  );
  const panelSessions = useMemo(() => {
    if (!unbound) return previewSessions;
    return [...previewSessions, unbound].sort((a, b) => a.updatedAt.localeCompare(b.updatedAt));
  }, [previewSessions, unbound]);
  const latestPreview = panelSessions.length ? panelSessions[panelSessions.length - 1] : null;
  const hasLiveTrace =
    panelSessions.length > 0 ||
    String(fallbackText || "").trim().length > 0 ||
    fallbackActivities.length > 0;

  if (!hasLiveTrace && rawEvents.length > 0) {
    return (
      <div className={`flex h-full ${minHeight} flex-col`}>
        <HeadlessRawTrace
          events={rawEvents}
          emptyLabel={emptyLabel}
          isDark={isDark}
          compact={compact}
          className={`h-full ${minHeight} text-left text-[var(--color-text-secondary)]`}
        />
      </div>
    );
  }

  const liveTrace = (
    <HeadlessLiveTrace
      previewSessions={panelSessions}
      fallbackText={fallbackText}
      fallbackActivities={fallbackActivities}
      fallbackUpdatedAt={String(latestPreview?.updatedAt || "").trim()}
      fallbackPendingEventId={String(latestPreview?.pendingEventId || `preview:${actorId}`).trim()}
      fallbackStreamId={String(latestPreview?.currentStreamId || "").trim()}
      fallbackStreamPhase={String(latestPreview?.streamPhase || "")
        .trim()
        .toLowerCase()}
      fallbackPhase={String(latestPreview?.phase || "")
        .trim()
        .toLowerCase()}
      emptyLabel={emptyLabel}
      isDark={isDark}
      density={compact ? "compact" : "expanded"}
      className={classNames(
        "h-full overflow-y-auto scrollbar-hide text-left text-[var(--color-text-secondary)]",
        minHeight,
      )}
    />
  );

  return <div className={`flex h-full ${minHeight} flex-col`}>{liveTrace}</div>;
}
