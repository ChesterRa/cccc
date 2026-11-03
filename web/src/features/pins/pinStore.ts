import { useEffect } from "react";
import { create } from "zustand";
import { fetchPinnedIds, setMessagePinned, type PinnedId } from "../../services/api/pins";

export type GroupPins = {
  /** Pinned event IDs in the server's order. */
  ids: string[];
  pinned: Record<string, true>;
  loaded: boolean;
  /** The running daemon has no pins: the tab and the action stay hidden. */
  unsupported: boolean;
};

type PinState = {
  groups: Record<string, GroupPins>;
  replace: (groupId: string, pins: PinnedId[]) => void;
  markUnsupported: (groupId: string) => void;
};

export const usePinStore = create<PinState>((set) => ({
  groups: {},
  replace: (groupId, pins) =>
    set((state) => {
      const ids = pins.map((pin) => pin.event_id);
      return {
        groups: {
          ...state.groups,
          [groupId]: {
            ids,
            pinned: Object.fromEntries(ids.map((id) => [id, true as const])),
            loaded: true,
            unsupported: false,
          },
        },
      };
    }),
  markUnsupported: (groupId) =>
    set((state) => ({
      groups: {
        ...state.groups,
        [groupId]: { ids: [], pinned: {}, loaded: true, unsupported: true },
      },
    })),
}));

// The daemon applies changes in the order they reach it, and each change answers with the
// whole list. A Group's changes are therefore sent one at a time, so their answers arrive in
// the order the daemon applied them, and each can be shown. A read is shown only if no change
// was under way or finished meanwhile, since a change's answer is at least as new, and if no
// read started after it has been shown. The second rule is best effort: when another device's
// change lands between two overlapping reads, the one sent later usually saw it, but nothing
// orders them at the daemon. The next read, on opening the Group or returning to the page,
// corrects either way.
type Sync = {
  queue: Promise<unknown>;
  inFlight: number;
  done: number;
  reads: number;
  shownRead: number;
};
const syncs = new Map<string, Sync>();
const loading = new Map<string, Promise<void>>();

function syncOf(groupId: string): Sync {
  let entry = syncs.get(groupId);
  if (!entry) {
    entry = { queue: Promise.resolve(), inFlight: 0, done: 0, reads: 0, shownRead: 0 };
    syncs.set(groupId, entry);
  }
  return entry;
}

export type PinRead = { done: number; read: number };

/** Settles once the Group's changes sent so far have been answered. */
export function afterPinWrites(groupId: string): Promise<void> {
  return syncOf(groupId).queue.then(() => undefined);
}

/** Mark the start of a read of a Group's pins made elsewhere, such as the Pinned view's. */
export function beginPinRead(groupId: string): PinRead {
  const entry = syncOf(groupId);
  entry.reads += 1;
  return { done: entry.done, read: entry.reads };
}

/** Show that read's pins unless a change, or a later read, has been seen since it started. */
export function applyPinRead(groupId: string, started: PinRead, pins: PinnedId[]): boolean {
  const entry = syncOf(groupId);
  if (entry.inFlight > 0 || entry.done !== started.done || started.read < entry.shownRead)
    return false;
  entry.shownRead = started.read;
  usePinStore.getState().replace(groupId, pins);
  return true;
}

/** Read the Group's pinned IDs; overlapping calls share one request. */
export function loadPinnedIds(groupId: string): Promise<void> {
  const current = loading.get(groupId);
  if (current) return current;
  const started = beginPinRead(groupId);
  const done = fetchPinnedIds(groupId)
    .then((response) => {
      if (response.ok) applyPinRead(groupId, started, response.result.pins ?? []);
      else if (response.error.code === "unknown_op")
        usePinStore.getState().markUnsupported(groupId);
      // Any other failure keeps what is shown; the next visit or return to the page retries.
    })
    .finally(() => loading.delete(groupId));
  loading.set(groupId, done);
  return done;
}

export type PinChangeResult = { ok: true } | { ok: false; code: string; message: string };

export function setPinned(
  groupId: string,
  eventId: string,
  pinned: boolean,
): Promise<PinChangeResult> {
  const entry = syncOf(groupId);
  entry.inFlight += 1;
  const change = entry.queue
    .then(() => setMessagePinned(groupId, eventId, pinned))
    .then((response): PinChangeResult => {
      if (!response.ok)
        return { ok: false, code: response.error.code, message: response.error.message };
      usePinStore.getState().replace(groupId, response.result.pins ?? []);
      return { ok: true };
    })
    .finally(() => {
      entry.inFlight -= 1;
      entry.done += 1;
    });
  entry.queue = change.catch(() => undefined);
  // A refused or unanswered change may have set aside a read that overlapped it, and its
  // outcome at the daemon may be unknown, so read once more after the Group's queued changes
  // and any read under way have settled. Only a failed change reads again, never a read.
  void change.then((result) => {
    if (!result.ok)
      void afterPinWrites(groupId)
        .then(() => loading.get(groupId))
        .then(() => loadPinnedIds(groupId));
  });
  return change;
}

/**
 * Keep the open Group's pins current: read them when the Group opens and again whenever the
 * page becomes visible, since another browser or device may have changed them meanwhile.
 */
export function usePinnedIds(groupId: string, enabled: boolean): GroupPins | undefined {
  const pins = usePinStore((state) => (groupId ? state.groups[groupId] : undefined));
  useEffect(() => {
    if (!enabled || !groupId) return;
    void loadPinnedIds(groupId);
    const refresh = () => {
      if (document.visibilityState === "visible") void loadPinnedIds(groupId);
    };
    document.addEventListener("visibilitychange", refresh);
    return () => document.removeEventListener("visibilitychange", refresh);
  }, [enabled, groupId]);
  return enabled ? pins : undefined;
}

export function useMessagePinned(groupId: string, eventId: string | undefined): boolean {
  return usePinStore((state) => (eventId ? !!state.groups[groupId]?.pinned[eventId] : false));
}
