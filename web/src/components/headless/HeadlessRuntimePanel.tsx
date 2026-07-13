import type { HeadlessPreviewSession, HeadlessStreamEvent, StreamingActivity } from "../../types";
import { classNames } from "../../utils/classNames";
import { HeadlessLiveTrace } from "./HeadlessLiveTrace";
import { HeadlessRawTrace } from "./HeadlessRawTrace";

type HeadlessRuntimePanelProps = {
  actorId: string;
  previewSessions: HeadlessPreviewSession[];
  fallbackText: string;
  fallbackActivities: StreamingActivity[];
  rawEvents: HeadlessStreamEvent[];
  emptyLabel: string;
  isDark: boolean;
};

export function HeadlessRuntimePanel({
  actorId,
  previewSessions,
  fallbackText,
  fallbackActivities,
  rawEvents,
  emptyLabel,
  isDark,
}: HeadlessRuntimePanelProps) {
  const latestPreview =
    previewSessions.length > 0 ? previewSessions[previewSessions.length - 1] : null;
  const hasLiveTrace =
    previewSessions.length > 0 ||
    String(fallbackText || "").trim().length > 0 ||
    fallbackActivities.length > 0;

  if (!hasLiveTrace && rawEvents.length > 0) {
    return (
      <div className="flex h-full min-h-[420px] flex-col">
        <HeadlessRawTrace
          events={rawEvents}
          emptyLabel={emptyLabel}
          isDark={isDark}
          className="h-full min-h-[420px] text-left text-[var(--color-text-secondary)]"
        />
      </div>
    );
  }

  const liveTrace = (
    <HeadlessLiveTrace
      previewSessions={previewSessions}
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
      density="expanded"
      className={classNames(
        "h-full min-h-[420px] overflow-y-auto scrollbar-hide text-left text-[var(--color-text-secondary)]",
      )}
    />
  );

  return <div className="flex h-full min-h-[420px] flex-col">{liveTrace}</div>;
}
