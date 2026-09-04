import { useMemo } from "react";
import type { Actor, LedgerEvent } from "../../types";
import type { GroupChatBucket } from "../../stores/groupStoreCore";
import { buildLiveWorkCards, type LiveWorkCard } from "./liveWorkCards";

export function useRuntimeDockWorkCards(args: {
  actors: Actor[];
  events: LedgerEvent[];
  bucket?: GroupChatBucket;
}): LiveWorkCard[] {
  return useMemo(
    () =>
      buildLiveWorkCards({
        actors: args.actors,
        events: args.events,
        latestActorPreviewByActorId: args.bucket?.latestActorPreviewByActorId || {},
        previewSessionsByActorId: args.bucket?.previewSessionsByActorId || {},
        latestActorTextByActorId: args.bucket?.latestActorTextByActorId || {},
        latestActorActivitiesByActorId: args.bucket?.latestActorActivitiesByActorId || {},
        replySessionsByPendingEventId: args.bucket?.replySessionsByPendingEventId || {},
      }),
    [args.actors, args.bucket, args.events],
  );
}
