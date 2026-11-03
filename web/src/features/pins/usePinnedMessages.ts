import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  shouldShowInConversation,
  toVisibleConversationEvent,
} from "../../hooks/chat/chatTabBasics";
import { fetchPinnedMessages } from "../../services/api/pins";
import type { LedgerEvent } from "../../types";
import { afterPinWrites, applyPinRead, beginPinRead, usePinStore } from "./pinStore";

type ViewState = {
  key: string;
  events: LedgerEvent[];
  /** Pins whose message can no longer be read. */
  unavailable: string[];
  loaded: boolean;
  loading: boolean;
  error: boolean;
};

function empty(key: string): ViewState {
  return { key, events: [], unavailable: [], loaded: false, loading: false, error: false };
}

/**
 * The messages shown under Pinned, read in full from the server, since most of them are no
 * longer in the loaded part of the conversation. A message unpinned here stays in the view
 * until the user leaves it, so a mistaken unpin can be undone in place; the view reads the
 * list again when it opens and when the page becomes visible, after any change still under way
 * has been answered, so a message pinned just before opening it is listed. If reading it again
 * fails, the view keeps what it showed, under the error and its retry.
 */
export function usePinnedMessages(groupId: string, active: boolean) {
  const key = active && groupId ? groupId : "";
  const [state, setState] = useState<ViewState>(() => empty(key));
  const visit = useRef(0);

  const load = useCallback(() => {
    if (!key) return;
    const request = ++visit.current;
    setState((current) => ({
      ...(current.key === key ? current : empty(key)),
      loading: true,
      error: false,
    }));
    void afterPinWrites(key).then(async () => {
      if (request !== visit.current) return;
      const started = beginPinRead(key);
      const response = await fetchPinnedMessages(key);
      if (request !== visit.current) return;
      if (!response.ok) {
        setState((current) => ({ ...current, loading: false, error: true }));
        return;
      }
      const pins = response.result.pins ?? [];
      applyPinRead(key, started, pins);
      setState({
        key,
        // Shown as the conversation shows them: a slash command keeps its short form.
        events: pins.flatMap((pin) =>
          pin.event && shouldShowInConversation(pin.event)
            ? [toVisibleConversationEvent(pin.event)]
            : [],
        ),
        unavailable: pins.filter((pin) => !pin.event).map((pin) => pin.event_id),
        loaded: true,
        loading: false,
        error: false,
      });
    });
  }, [key]);

  useEffect(() => {
    if (!key) {
      visit.current += 1;
      setState(empty(""));
      return;
    }
    load();
    const refresh = () => {
      if (document.visibilityState === "visible") load();
    };
    document.addEventListener("visibilitychange", refresh);
    return () => document.removeEventListener("visibilitychange", refresh);
  }, [key, load]);

  const current = state.key === key ? state : empty(key);
  // An unreadable pin leaves the notice once it is removed.
  const pinned = usePinStore((store) => (key ? store.groups[key]?.pinned : undefined));
  const unavailable = useMemo(
    () => current.unavailable.filter((id) => !!pinned?.[id]),
    [current.unavailable, pinned],
  );
  return {
    active: !!key,
    events: current.events,
    unavailable,
    loaded: current.loaded,
    loading: current.loading || (!!key && !current.loaded && !current.error),
    error: current.error,
    refresh: load,
  };
}
