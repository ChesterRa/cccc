import { useEffect } from "react";
import { useGroupStore } from "../stores";
import { startupInputGeneration } from "./useStartupInputWaiting";

// Shorter than the delay before a wait is shown, so an ordinary start clears first.
const STARTUP_INPUT_REFRESH_MS = 3000;

/** No event reports a terminal reaching its chat input, so while a member of the shown Group
 *  waits, its Actors are read again on a short interval. The reads stop once none waits and
 *  start no runtime work. */
export function useStartupInputRefresh(groupId: string) {
  const waiting = useGroupStore(
    (state) =>
      Boolean(groupId) &&
      state.selectedGroupId === groupId &&
      state.actors.some((actor) => Boolean(startupInputGeneration(actor))),
  );
  const refreshActors = useGroupStore((state) => state.refreshActors);
  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => {
      void refreshActors(groupId, { includeUnread: false });
    }, STARTUP_INPUT_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [waiting, groupId, refreshActors]);
}
