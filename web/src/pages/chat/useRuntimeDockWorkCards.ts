import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, LedgerEvent, StreamingActivity } from "../../types";
import type { GroupChatBucket } from "../../stores/groupStoreCore";
import { useRuntimeActivityStore } from "../../stores";
import { useRuntimeActivityStream } from "../../hooks/useRuntimeActivityStream";
import { buildLiveWorkCards, type LiveWorkCard } from "./liveWorkCards";
import { projectRuntimeActivities } from "./runtimeActivityProjection";

const EMPTY_RUNTIME_EVENTS = {};

export function useRuntimeDockWorkCards(args: {
  groupId: string;
  actors: Actor[];
  events: LedgerEvent[];
  bucket?: GroupChatBucket;
}): LiveWorkCard[] {
  const groupId = String(args.groupId || "").trim();
  useRuntimeActivityStream(groupId);
  const { t } = useTranslation("actors");
  const runtimeEventsByActorId = useRuntimeActivityStore(
    (state) => state.byGroup[groupId] || EMPTY_RUNTIME_EVENTS,
  );
  const runtimeActivitiesByActorId = useMemo(
    () =>
      Object.fromEntries(
        Object.entries(runtimeEventsByActorId).map(([actorId, events]) => [
          actorId,
          projectRuntimeActivities(events, (key, options) => t(key, options)),
        ]),
      ) as Record<string, StreamingActivity[]>,
    [runtimeEventsByActorId, t],
  );
  return useMemo(
    () =>
      buildLiveWorkCards({
        actors: args.actors,
        events: args.events,
        latestActorPreviewByActorId: args.bucket?.latestActorPreviewByActorId || {},
        previewSessionsByActorId: args.bucket?.previewSessionsByActorId || {},
        latestActorTextByActorId: args.bucket?.latestActorTextByActorId || {},
        latestActorActivitiesByActorId: args.bucket?.latestActorActivitiesByActorId || {},
        runtimeActivitiesByActorId,
        replySessionsByPendingEventId: args.bucket?.replySessionsByPendingEventId || {},
      }),
    [args.actors, args.bucket, args.events, runtimeActivitiesByActorId],
  );
}
