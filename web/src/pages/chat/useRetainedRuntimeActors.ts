import { useEffect, useState, type ReactElement, type ReactNode } from "react";
import type { Actor } from "../../types";
import { getEffectiveActorRunner } from "../../utils/headlessRuntimeSupport";

export const RETAINED_TERMINAL_LIMIT = 32;
export const RETAINED_TERMINAL_TTL_MS = 5 * 60 * 1000;

export type RuntimeActorView = {
  isVisible: boolean;
  compact: boolean;
  onExpand: () => void;
  navigation?: ReactNode;
  onPage?: (direction: -1 | 1) => void;
  isDark?: boolean;
  isSmallScreen?: boolean;
  readOnly?: boolean;
};
export type RuntimeActorRenderer = (
  actorId: string,
  view: RuntimeActorView,
) => ReactElement<RuntimeActorView>;

type Entry = {
  key: string;
  groupId: string;
  actorId: string;
  generation: string;
  element: ReactElement<RuntimeActorView>;
  retain: boolean;
  hiddenAt: number | null;
};
type Inputs = {
  groupId: string;
  actors: Actor[];
  visibleIds: string;
  groupIds: string;
  loading: boolean;
  readOnly: boolean;
  renderActor: RuntimeActorRenderer;
};
const NOOP = () => {};

function trim(entries: Entry[], now: number): Entry[] {
  const hidden = entries
    .filter((entry) => entry.hiddenAt !== null && now - entry.hiddenAt < RETAINED_TERMINAL_TTL_MS)
    .sort((a, b) => b.hiddenAt! - a.hiddenAt!)
    .slice(0, RETAINED_TERMINAL_LIMIT);
  const keep = new Set(hidden);
  return entries.filter((entry) => entry.hiddenAt === null || keep.has(entry));
}

function reconcile(previous: Entry[], inputs: Inputs, now: number): Entry[] {
  const visible = new Set<string>(JSON.parse(inputs.visibleIds));
  const groups = new Set<string>(JSON.parse(inputs.groupIds));
  const actors = new Map(inputs.actors.map((actor) => [actor.id, actor]));
  const entries = new Map<string, Entry>();
  const makeEntry = (id: string, actor: Actor | undefined, hiddenAt: number | null): Entry => {
    const generation = actor?.generation || "";
    return {
      key: JSON.stringify([inputs.groupId, id, generation]),
      groupId: inputs.groupId,
      actorId: id,
      generation,
      element: inputs.renderActor(id, { isVisible: false, compact: true, onExpand: NOOP }),
      retain: !!actor && actor.running !== false && getEffectiveActorRunner(actor) === "pty",
      hiddenAt,
    };
  };
  for (const old of previous) {
    if (!groups.has(old.groupId)) continue;
    const current = old.groupId === inputs.groupId;
    const actor = current ? actors.get(old.actorId) : undefined;
    // An incomplete Group hydration must not discard an intact terminal.
    if (current && !inputs.loading && (!actor || (actor.generation || "") !== old.generation))
      continue;
    const visibleHere = current && visible.has(old.actorId);
    const hiddenAt = visibleHere ? null : (old.hiddenAt ?? now);
    const entry = current && actor ? makeEntry(old.actorId, actor, hiddenAt) : { ...old, hiddenAt };
    if (visibleHere || entry.retain) entries.set(entry.key, entry);
  }
  for (const id of visible) {
    if (!inputs.groupId) continue;
    const actor = actors.get(id);
    if (
      !actor &&
      inputs.loading &&
      [...entries.values()].some(
        (entry) => entry.groupId === inputs.groupId && entry.actorId === id,
      )
    )
      continue;
    const entry = makeEntry(id, actor, null);
    entries.set(entry.key, entry);
  }
  return trim([...entries.values()], now);
}

/** Retain mounted Actor views, not serialized terminal buffers or detached transports. */
export function useRetainedRuntimeActors(
  args: Omit<Inputs, "visibleIds" | "groupIds"> & {
    visibleActorIds: string[];
    availableGroupIds: string[];
  },
) {
  const inputs: Inputs = {
    groupId: args.groupId,
    actors: args.actors,
    loading: args.loading,
    readOnly: args.readOnly,
    renderActor: args.renderActor,
    visibleIds: JSON.stringify(args.visibleActorIds),
    groupIds: JSON.stringify(args.availableGroupIds),
  };
  const [state, setState] = useState(() => ({
    inputs,
    entries: reconcile([], inputs, Date.now()),
  }));
  // Adjust before committing children, so a Group switch never briefly renders
  // a cached Actor with another Group's identity or callbacks.
  if (
    state.inputs.groupId !== inputs.groupId ||
    state.inputs.actors !== inputs.actors ||
    state.inputs.loading !== inputs.loading ||
    state.inputs.readOnly !== inputs.readOnly ||
    state.inputs.renderActor !== inputs.renderActor ||
    state.inputs.visibleIds !== inputs.visibleIds ||
    state.inputs.groupIds !== inputs.groupIds
  ) {
    setState({
      inputs,
      entries: reconcile(
        state.inputs.readOnly === inputs.readOnly ? state.entries : [],
        inputs,
        Date.now(),
      ),
    });
  }

  useEffect(() => {
    const deadlines = state.entries.flatMap((entry) =>
      entry.hiddenAt === null ? [] : [entry.hiddenAt + RETAINED_TERMINAL_TTL_MS],
    );
    if (!deadlines.length) return;
    const timer = window.setTimeout(
      () => {
        setState((current) => ({ ...current, entries: trim(current.entries, Date.now()) }));
      },
      Math.max(0, Math.min(...deadlines) - Date.now()),
    );
    return () => window.clearTimeout(timer);
  }, [state.entries]);
  return state.entries;
}
