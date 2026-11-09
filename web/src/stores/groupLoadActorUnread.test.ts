// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";

vi.mock("../services/api", () => ({
  fetchGroup: vi.fn(),
  fetchLedgerTail: vi.fn(),
  fetchActors: vi.fn(),
  fetchContext: vi.fn(),
  fetchSettings: vi.fn(),
  fetchPresentation: vi.fn(),
  fetchLedgerStatuses: vi.fn(),
}));

import * as api from "../services/api";
import type { Actor, GroupMeta } from "../types";
import {
  deferredUnreadRefreshTimers,
  groupViewCache,
  loadGroupInFlight,
  refreshActorsInFlight,
  refreshActorsQueued,
  saveGroupView,
} from "./groupStoreCore";
import { useGroupStore } from "./useGroupStore";

const GROUP = "g_mail";
const original = useGroupStore.getState();

function worker(unread?: number): Actor {
  return {
    id: "worker",
    role: "peer",
    runtime: "codex",
    runner: "headless",
    running: true,
    ...(unread === undefined ? {} : { unread_count: unread }),
  };
}

afterEach(() => {
  useGroupStore.setState(original, true);
  groupViewCache.delete(GROUP);
  loadGroupInFlight.clear();
  refreshActorsInFlight.clear();
  refreshActorsQueued.clear();
  for (const timer of deferredUnreadRefreshTimers.values()) clearTimeout(timer);
  deferredUnreadRefreshTimers.clear();
  vi.clearAllMocks();
});

it("keeps the shown unread count while a revisited Group's first actor read has none", async () => {
  saveGroupView(GROUP, { actors: [worker(3)] });
  vi.mocked(api.fetchGroup).mockResolvedValue({
    ok: true,
    result: { group: { group_id: GROUP, title: "Mail" } },
  } as never);
  vi.mocked(api.fetchLedgerTail).mockResolvedValue({
    ok: true,
    result: { events: [], has_more: false },
  } as never);
  // The first read leaves unread counts out; the deferred refresh asks for them.
  vi.mocked(api.fetchActors).mockImplementation(
    async (_groupId: string, includeUnread?: boolean) =>
      ({ ok: true, result: { actors: [worker(includeUnread ? 4 : undefined)] } }) as never,
  );
  for (const read of [api.fetchContext, api.fetchSettings, api.fetchPresentation]) {
    vi.mocked(read).mockResolvedValue({ ok: false } as never);
  }
  vi.mocked(api.fetchLedgerStatuses).mockResolvedValue({
    ok: true,
    result: { statuses: {} },
  } as never);
  useGroupStore.setState({
    groups: [{ group_id: GROUP, title: "Mail" } as GroupMeta],
    selectedGroupId: GROUP,
    groupDoc: null,
    actors: [],
  });

  const seen: Array<{ actors: Actor[]; unread: number | undefined }> = [];
  const unsubscribe = useGroupStore.subscribe((state) =>
    seen.push({ actors: state.actors, unread: state.actors[0]?.unread_count }),
  );
  try {
    await useGroupStore.getState().loadGroup(GROUP);
    await vi.waitFor(() => expect(useGroupStore.getState().actors[0]?.unread_count).toBe(4));
  } finally {
    unsubscribe();
  }

  expect(vi.mocked(api.fetchActors).mock.calls.map((call) => call[1])).toEqual([false, true]);
  const refreshed = seen.findIndex((entry) => entry.unread === 4);
  expect(refreshed).toBeGreaterThan(1);
  // The selection shows the cached count; until fresh counts arrive, no update replaces
  // that actor array, so the mail mark neither changes nor renders again.
  const primed = seen[0].actors;
  for (const entry of seen.slice(0, refreshed)) {
    expect(entry.unread).toBe(3);
    expect(entry.actors).toBe(primed);
  }
});
