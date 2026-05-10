import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { LedgerEvent } from "../../src/types";

vi.mock("../../src/services/api", () => ({
  fetchActors: vi.fn(),
  fetchGroup: vi.fn(),
  fetchLedgerTail: vi.fn(),
  fetchLedgerStatuses: vi.fn(),
  fetchOlderMessages: vi.fn(),
  fetchMessageWindow: vi.fn(),
  fetchContext: vi.fn(),
  fetchSettings: vi.fn(),
  fetchPresentation: vi.fn(),
  fetchGroups: vi.fn(),
}));

function makeStorage() {
  const data = new Map<string, string>();
  return {
    getItem: vi.fn((key: string) => data.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      data.set(key, String(value));
    }),
    removeItem: vi.fn((key: string) => {
      data.delete(key);
    }),
    clear: vi.fn(() => {
      data.clear();
    }),
  };
}

const localStorageMock = makeStorage();
vi.stubGlobal("localStorage", localStorageMock);
vi.stubGlobal("window", { setTimeout, clearTimeout });

let useGroupStore: typeof import("../../src/stores/useGroupStore").useGroupStore;
let useComposerStore: typeof import("../../src/stores/useComposerStore").useComposerStore;
let api: typeof import("../../src/services/api");
let groupStoreCore: typeof import("../../src/stores/groupStoreCore");
const SELECTED_GROUP_ID_KEY = "cccc-selected-group-id";
const ARCHIVED_GROUP_IDS_KEY = "cccc-archived-group-ids";

async function flushDeferredUnreadRefresh() {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

async function importFreshStore() {
  vi.resetModules();
  api = await import("../../src/services/api");
  groupStoreCore = await import("../../src/stores/groupStoreCore");
  ({ useComposerStore } = await import("../../src/stores/useComposerStore"));
  const mod = await import("../../src/stores/useGroupStore");
  useGroupStore = mod.useGroupStore;
  return mod;
}

beforeAll(async () => {
  api = await import("../../src/services/api");
  groupStoreCore = await import("../../src/stores/groupStoreCore");
  ({ useComposerStore } = await import("../../src/stores/useComposerStore"));
  ({ useGroupStore } = await import("../../src/stores/useGroupStore"));
});

describe("useGroupStore selection and archive persistence", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorageMock.clear();
    groupStoreCore.groupViewCache.clear();
    useComposerStore?.setState({
      activeGroupId: "",
      composerText: "",
      composerFiles: [],
      toText: "",
      replyTarget: null,
      quotedPresentationRef: null,
      priority: "normal",
      replyRequired: false,
      destGroupId: "",
      drafts: {},
      normalToTextByGroup: {},
    });
  });

  it("initializes selectedGroupId from localStorage", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-2");
    const mod = await importFreshStore();
    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-2");
  });

  it("initializes composer ownership from persisted selected group before refresh succeeds", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-2");
    const mod = await importFreshStore();

    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-2");
    expect(useComposerStore.getState().activeGroupId).toBe("g-2");
    expect(useComposerStore.getState().destGroupId).toBe("g-2");
  });

  it("persists explicit group selection changes", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().setSelectedGroupId("g-9");
    expect(localStorageMock.getItem(SELECTED_GROUP_ID_KEY)).toBe("g-9");
    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-9");
  });

  it("switches composer state synchronously with selected group changes", async () => {
    const mod = await importFreshStore();
    useComposerStore.getState().switchGroup(null, "g-a");
    useComposerStore.getState().setToText("@all");
    useComposerStore.getState().setComposerText("draft for a");
    mod.useGroupStore.setState({ selectedGroupId: "g-a" });

    mod.useGroupStore.getState().setSelectedGroupId("g-b");

    const composerState = useComposerStore.getState();
    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-b");
    expect(composerState.activeGroupId).toBe("g-b");
    expect(composerState.destGroupId).toBe("g-b");
    expect(composerState.toText).toBe("");
    expect(composerState.drafts["g-a"]).toMatchObject({
      composerText: "draft for a",
      toText: "@all",
    });
  });

  it("refreshGroups prefers the persisted selection when current state is empty", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-2");
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: {
        groups: [
          { group_id: "g-1", title: "One", state: "idle", topic: "" },
          { group_id: "g-2", title: "Two", state: "active", topic: "" },
        ],
      },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);

    const mod = await importFreshStore();
    mod.useGroupStore.setState({ selectedGroupId: "" });
    await mod.useGroupStore.getState().refreshGroups();
    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-2");
  });

  it("refreshGroups synchronizes composer state when it auto-selects a persisted group", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-2");
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: {
        groups: [
          { group_id: "g-1", title: "One", state: "idle", topic: "" },
          { group_id: "g-2", title: "Two", state: "active", topic: "" },
        ],
      },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);

    const mod = await importFreshStore();
    mod.useGroupStore.setState({ selectedGroupId: "" });
    await mod.useGroupStore.getState().refreshGroups();

    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-2");
    expect(useComposerStore.getState().activeGroupId).toBe("g-2");
    expect(useComposerStore.getState().destGroupId).toBe("g-2");
  });

  it("refreshGroups synchronizes composer state when persisted selection already exists", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-2");
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: {
        groups: [
          { group_id: "g-1", title: "One", state: "idle", topic: "" },
          { group_id: "g-2", title: "Two", state: "active", topic: "" },
        ],
      },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);

    const mod = await importFreshStore();
    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-2");
    expect(useComposerStore.getState().activeGroupId).toBe("g-2");

    await mod.useGroupStore.getState().refreshGroups();

    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-2");
    expect(useComposerStore.getState().activeGroupId).toBe("g-2");
    expect(useComposerStore.getState().destGroupId).toBe("g-2");
  });

  it("refreshGroups falls back to the first group when the persisted one no longer exists", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-missing");
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: {
        groups: [
          { group_id: "g-1", title: "One", state: "idle", topic: "" },
          { group_id: "g-2", title: "Two", state: "active", topic: "" },
        ],
      },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);

    const mod = await importFreshStore();
    await mod.useGroupStore.getState().refreshGroups();
    expect(mod.useGroupStore.getState().selectedGroupId).toBe("g-1");
    expect(localStorageMock.getItem(SELECTED_GROUP_ID_KEY)).toBe("g-1");
  });

  it("clears the persisted selection when no groups remain", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-2");
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: { groups: [] },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);

    const mod = await importFreshStore();
    await mod.useGroupStore.getState().refreshGroups();
    expect(mod.useGroupStore.getState().selectedGroupId).toBe("");
    expect(localStorageMock.getItem(SELECTED_GROUP_ID_KEY)).toBeNull();
  });

  it("refreshGroups clears composer ownership when no groups remain", async () => {
    localStorageMock.setItem(SELECTED_GROUP_ID_KEY, "g-1");
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: { groups: [] },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);

    const mod = await importFreshStore();
    mod.useGroupStore.setState({ selectedGroupId: "g-1" });
    useComposerStore.getState().switchGroup(null, "g-1");
    useComposerStore.getState().setToText("@all");
    useComposerStore.getState().setComposerText("draft");

    await mod.useGroupStore.getState().refreshGroups();

    expect(mod.useGroupStore.getState().selectedGroupId).toBe("");
    expect(useComposerStore.getState().activeGroupId).toBe("");
    expect(useComposerStore.getState().destGroupId).toBe("");
  });

  it("initializes archivedGroupIds from localStorage", async () => {
    localStorageMock.setItem(ARCHIVED_GROUP_IDS_KEY, JSON.stringify(["g-2", "g-3"]));
    const mod = await importFreshStore();
    expect(mod.useGroupStore.getState().archivedGroupIds).toEqual(["g-2", "g-3"]);
  });

  it("cleans stale archived ids when the group list refreshes", async () => {
    localStorageMock.setItem(ARCHIVED_GROUP_IDS_KEY, JSON.stringify(["g-2", "g-missing"]));
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: {
        groups: [
          { group_id: "g-1", title: "One", state: "idle", topic: "" },
          { group_id: "g-2", title: "Two", state: "active", topic: "" },
        ],
      },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);

    const mod = await importFreshStore();
    await mod.useGroupStore.getState().refreshGroups();
    expect(mod.useGroupStore.getState().archivedGroupIds).toEqual(["g-2"]);
    expect(localStorageMock.getItem(ARCHIVED_GROUP_IDS_KEY)).toBe(JSON.stringify(["g-2"]));
  });

  it("persists archive and restore actions", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.setState({
      groups: [
        { group_id: "g-1", title: "One", state: "idle", topic: "" },
        { group_id: "g-2", title: "Two", state: "active", topic: "" },
      ],
      groupOrder: ["g-1", "g-2"],
    });

    mod.useGroupStore.getState().archiveGroup("g-2");
    expect(mod.useGroupStore.getState().archivedGroupIds).toEqual(["g-2"]);
    expect(localStorageMock.getItem(ARCHIVED_GROUP_IDS_KEY)).toBe(JSON.stringify(["g-2"]));

    mod.useGroupStore.getState().restoreGroup("g-2");
    expect(mod.useGroupStore.getState().archivedGroupIds).toEqual([]);
    expect(localStorageMock.getItem(ARCHIVED_GROUP_IDS_KEY)).toBeNull();
  });

  it("reorders only within the requested sidebar section", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.setState({
      groups: [
        { group_id: "g-1", title: "One", state: "idle", topic: "" },
        { group_id: "g-2", title: "Two", state: "active", topic: "" },
        { group_id: "g-3", title: "Three", state: "paused", topic: "" },
        { group_id: "g-4", title: "Four", state: "idle", topic: "" },
      ],
      groupOrder: ["g-1", "g-2", "g-3", "g-4"],
      archivedGroupIds: ["g-2", "g-4"],
    });

    mod.useGroupStore.getState().reorderGroupsInSection("working", 1, 0);
    expect(mod.useGroupStore.getState().groupOrder).toEqual(["g-3", "g-2", "g-1", "g-4"]);

    mod.useGroupStore.getState().reorderGroupsInSection("archived", 1, 0);
    expect(mod.useGroupStore.getState().groupOrder).toEqual(["g-3", "g-4", "g-1", "g-2"]);
  });

  it("projects cached runtime state for non-selected groups in ordered results", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.setState({
      groups: [
        { group_id: "g-1", title: "One", state: "active", running: false, topic: "" },
        { group_id: "g-2", title: "Two", state: "active", running: false, topic: "" },
      ],
      groupOrder: ["g-1", "g-2"],
      selectedGroupId: "g-1",
      groupDoc: { group_id: "g-1", state: "active", running: false },
      actors: [],
    });
    groupStoreCore.saveGroupView("g-2", {
      groupDoc: {
        group_id: "g-2",
        state: "active",
        runtime_status: {
          lifecycle_state: "active",
          runtime_running: false,
          running_actor_count: 0,
          has_running_foreman: false,
        },
      },
      actors: [{ id: "a-2", running: true }],
    });

    const ordered = mod.useGroupStore.getState().getOrderedGroups();
    expect(ordered[1]).toMatchObject({
      group_id: "g-2",
      running: true,
      state: "active",
      runtime_status: {
        lifecycle_state: "active",
        runtime_running: true,
      },
    });
  });

  it("does not let a stale cached groupDoc override refreshed non-selected group meta", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.setState({
      groups: [
        { group_id: "g-1", title: "One", state: "active", running: true, topic: "" },
        {
          group_id: "g-2",
          title: "Two",
          state: "idle",
          running: false,
          topic: "",
          runtime_status: {
            lifecycle_state: "idle",
            runtime_running: false,
            running_actor_count: 0,
            has_running_foreman: false,
          },
        },
      ],
      groupOrder: ["g-1", "g-2"],
      selectedGroupId: "g-1",
      groupDoc: { group_id: "g-1", state: "active", running: true },
      actors: [],
    });
    groupStoreCore.saveGroupView("g-2", {
      groupDoc: {
        group_id: "g-2",
        state: "active",
        running: true,
        runtime_status: {
          lifecycle_state: "active",
          runtime_running: true,
          running_actor_count: 1,
          has_running_foreman: true,
        },
      },
      actors: [],
    });

    const ordered = mod.useGroupStore.getState().getOrderedGroups();
    expect(ordered[1]).toMatchObject({
      group_id: "g-2",
      running: false,
      state: "idle",
      runtime_status: {
        lifecycle_state: "idle",
        runtime_running: false,
      },
    });
  });

  it("primes selected groupDoc with refreshed meta runtime over stale cached runtime", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.setState({
      groups: [
        { group_id: "g-1", title: "One", state: "active", running: true, topic: "" },
        {
          group_id: "g-2",
          title: "Two",
          state: "idle",
          running: false,
          topic: "",
          runtime_status: {
            lifecycle_state: "idle",
            runtime_running: false,
            running_actor_count: 0,
            has_running_foreman: false,
          },
        },
      ],
      groupOrder: ["g-1", "g-2"],
      selectedGroupId: "g-1",
      groupDoc: { group_id: "g-1", state: "active", running: true },
      actors: [],
    });
    groupStoreCore.saveGroupView("g-2", {
      groupDoc: {
        group_id: "g-2",
        title: "Cached Two",
        state: "active",
        running: true,
        runtime_status: {
          lifecycle_state: "active",
          runtime_running: true,
          running_actor_count: 1,
          has_running_foreman: true,
        },
      },
      actors: [],
    });

    mod.useGroupStore.getState().setSelectedGroupId("g-2");

    expect(mod.useGroupStore.getState().groupDoc).toMatchObject({
      group_id: "g-2",
      title: "Two",
      running: false,
      state: "idle",
      runtime_status: {
        lifecycle_state: "idle",
        runtime_running: false,
      },
    });
  });
});

describe("useGroupStore actors fetch policy", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorageMock.clear();
    useGroupStore.setState({
      groups: [{ group_id: "g-demo", title: "Demo", topic: "", state: "active" }],
      groupOrder: ["g-demo"],
      archivedGroupIds: [],
      selectedGroupId: "g-demo",
      chatByGroup: {},
      groupDoc: null,
      events: [],
      chatWindow: null,
      actors: [],
      groupContext: null,
      groupSettings: null,
      groupPresentation: null,
      runtimes: [],
      hasMoreHistory: true,
      isLoadingHistory: false,
      isChatWindowLoading: false,
    });

    vi.mocked(api.fetchActors).mockResolvedValue({ ok: true, result: { actors: [] } });
    vi.mocked(api.fetchGroup).mockImplementation(
      async (groupId: string) =>
        ({
          ok: true,
          result: {
            group: {
              group_id: groupId,
              title: String(groupId || ""),
              topic: "",
              state: "active",
            },
          },
        }) as Awaited<ReturnType<typeof api.fetchGroup>>,
    );
    vi.mocked(api.fetchLedgerTail).mockResolvedValue({
      ok: true,
      result: { events: [], has_more: false },
    } as Awaited<ReturnType<typeof api.fetchLedgerTail>>);
    vi.mocked(api.fetchLedgerStatuses).mockResolvedValue({
      ok: true,
      result: { statuses: {} },
    } as Awaited<ReturnType<typeof api.fetchLedgerStatuses>>);
    vi.mocked(api.fetchOlderMessages).mockResolvedValue({
      ok: true,
      result: { events: [], has_more: false, count: 0 },
    } as Awaited<ReturnType<typeof api.fetchOlderMessages>>);
    vi.mocked(api.fetchMessageWindow).mockResolvedValue({
      ok: true,
      result: {
        center_id: "msg-1",
        center_index: 0,
        events: [],
        has_more_before: false,
        has_more_after: false,
      },
    } as Awaited<ReturnType<typeof api.fetchMessageWindow>>);
    vi.mocked(api.fetchContext).mockResolvedValue({
      ok: true,
      result: null,
    } as Awaited<ReturnType<typeof api.fetchContext>>);
    vi.mocked(api.fetchSettings).mockResolvedValue({
      ok: true,
      result: { settings: null },
    } as Awaited<ReturnType<typeof api.fetchSettings>>);
    vi.mocked(api.fetchPresentation).mockResolvedValue({
      ok: true,
      result: {
        group_id: "g-demo",
        presentation: { v: 1, updated_at: "", highlight_slot_id: "", slots: [] },
      },
    } as Awaited<ReturnType<typeof api.fetchPresentation>>);
    vi.mocked(api.fetchGroups).mockResolvedValue({
      ok: true,
      result: { groups: [] },
    } as Awaited<ReturnType<typeof api.fetchGroups>>);
  });

  it("refreshActors explicitly requests unread counts by default", async () => {
    await useGroupStore.getState().refreshActors("g-demo");
    expect(api.fetchActors).toHaveBeenCalledWith("g-demo", true);
  });

  it("refreshActors can do pure-read refresh without wiping existing unread counts", async () => {
    useGroupStore.setState({
      actors: [{ id: "peer-1", unread_count: 4, running: true }],
    });
    vi.mocked(api.fetchActors).mockResolvedValue({
      ok: true,
      result: { actors: [{ id: "peer-1", running: false }] },
    } as Awaited<ReturnType<typeof api.fetchActors>>);

    await useGroupStore.getState().refreshActors("g-demo", { includeUnread: false });

    expect(api.fetchActors).toHaveBeenCalledWith("g-demo", false);
    expect(useGroupStore.getState().actors).toEqual([{ id: "peer-1", running: false, unread_count: 4 }]);
  });

  it("filterUiEvents drops runtime-only ledger events from chat tail", () => {
    const chatEvent: LedgerEvent = { id: "msg-1", kind: "chat.message", by: "user", data: { text: "hello" } };

    expect(groupStoreCore.filterUiEvents([
      { id: "ctx-1", kind: "context.sync", by: "system", data: {} },
      { id: "runtime-1", kind: "actor.activity", by: "system", data: { actors: [] } },
      chatEvent,
    ])).toEqual([chatEvent]);
  });

  it("updateActorActivity merges effective working state fields", () => {
    useGroupStore.setState({
      actors: [{ id: "peer-1", unread_count: 4, running: true, idle_seconds: 8 }],
    });

    useGroupStore.getState().updateActorActivity([
      {
        id: "peer-1",
        running: true,
        idle_seconds: 2,
        effective_working_state: "working",
        effective_working_reason: "agent_active_task",
        effective_active_task_id: "T1",
      },
    ]);

    expect(useGroupStore.getState().actors).toEqual([
      {
        id: "peer-1",
        unread_count: 4,
        running: true,
        idle_seconds: 2,
        effective_working_state: "working",
        effective_working_reason: "agent_active_task",
        effective_working_updated_at: null,
        effective_active_task_id: "T1",
      },
    ]);
  });

  it("setActors persists the selected-group actor snapshot across group switches", () => {
    useGroupStore.setState({
      groups: [
        { group_id: "g-demo", title: "Demo", topic: "", state: "active" },
        { group_id: "g-other", title: "Other", topic: "", state: "idle" },
      ],
      groupOrder: ["g-demo", "g-other"],
      selectedGroupId: "g-demo",
      actors: [],
    });

    useGroupStore.getState().setActors([{ id: "peer-1", running: true, unread_count: 2 }]);
    useGroupStore.getState().setSelectedGroupId("g-other");
    useGroupStore.getState().setSelectedGroupId("g-demo");

    expect(useGroupStore.getState().actors).toEqual([{ id: "peer-1", running: true, unread_count: 2 }]);
  });

  it("incrementActorUnread persists speculative unread counts across group switches", () => {
    useGroupStore.setState({
      groups: [
        { group_id: "g-demo", title: "Demo", topic: "", state: "active" },
        { group_id: "g-other", title: "Other", topic: "", state: "idle" },
      ],
      groupOrder: ["g-demo", "g-other"],
      selectedGroupId: "g-demo",
      actors: [{ id: "peer-1", running: true, unread_count: 2 }],
    });

    useGroupStore.getState().incrementActorUnread(["peer-1"]);
    useGroupStore.getState().setSelectedGroupId("g-other");
    useGroupStore.getState().setSelectedGroupId("g-demo");

    expect(useGroupStore.getState().actors).toEqual([{ id: "peer-1", running: true, unread_count: 3 }]);
  });

  it("updateActorActivity persists the latest working-state truth into the selected-group snapshot", () => {
    useGroupStore.setState({
      groups: [
        { group_id: "g-demo", title: "Demo", topic: "", state: "active" },
        { group_id: "g-other", title: "Other", topic: "", state: "idle" },
      ],
      groupOrder: ["g-demo", "g-other"],
      selectedGroupId: "g-demo",
      actors: [{ id: "peer-1", unread_count: 4, running: true, idle_seconds: 8 }],
    });

    useGroupStore.getState().updateActorActivity([
      {
        id: "peer-1",
        running: true,
        idle_seconds: 2,
        effective_working_state: "working",
        effective_working_reason: "agent_active_task",
        effective_active_task_id: "T1",
      },
    ]);

    useGroupStore.getState().setSelectedGroupId("g-other");
    useGroupStore.getState().setSelectedGroupId("g-demo");

    expect(useGroupStore.getState().actors).toEqual([
      {
        id: "peer-1",
        unread_count: 4,
        running: true,
        idle_seconds: 2,
        effective_working_state: "working",
        effective_working_reason: "agent_active_task",
        effective_working_updated_at: null,
        effective_active_task_id: "T1",
      },
    ]);
  });

  it("setGroupContext persists the selected-group context snapshot across group switches", () => {
    useGroupStore.setState({
      groups: [
        { group_id: "g-demo", title: "Demo", topic: "", state: "active" },
        { group_id: "g-other", title: "Other", topic: "", state: "idle" },
      ],
      groupOrder: ["g-demo", "g-other"],
      selectedGroupId: "g-demo",
      groupContext: null,
    });

    useGroupStore.getState().setGroupContext({ version: "ctxv:2", agent_states: [{ id: "peer-1" }] } as never);
    useGroupStore.getState().setSelectedGroupId("g-other");
    useGroupStore.getState().setSelectedGroupId("g-demo");

    expect(useGroupStore.getState().groupContext?.version).toBe("ctxv:2");
    expect(useGroupStore.getState().groupContext?.agent_states?.[0]?.id).toBe("peer-1");
  });

  it("loadGroup keeps unread counts on the selected group path", async () => {
    await useGroupStore.getState().loadGroup("g-demo");
    await vi.waitFor(() => {
      expect(api.fetchActors).toHaveBeenNthCalledWith(1, "g-demo", false);
      expect(api.fetchActors).toHaveBeenNthCalledWith(2, "g-demo", true);
    });
  });

  it("loadGroup waits for pure-read actors before scheduling unread refresh", async () => {
    let resolvePureRead: ((value: Awaited<ReturnType<typeof api.fetchActors>>) => void) | null = null;
    vi.mocked(api.fetchActors).mockImplementation((groupId: string, includeUnread = false) => {
      if (includeUnread) {
        return Promise.resolve({ ok: true, result: { actors: [{ id: "peer-1", unread_count: 3 }] } }) as ReturnType<typeof api.fetchActors>;
      }
      return new Promise((resolve) => {
        resolvePureRead = resolve as (value: Awaited<ReturnType<typeof api.fetchActors>>) => void;
      }) as ReturnType<typeof api.fetchActors>;
    });

    await useGroupStore.getState().loadGroup("g-demo");
    await flushDeferredUnreadRefresh();
    expect(api.fetchActors).toHaveBeenCalledTimes(1);
    expect(api.fetchActors).toHaveBeenCalledWith("g-demo", false);

    resolvePureRead?.({ ok: true, result: { actors: [{ id: "peer-1" }] } } as Awaited<ReturnType<typeof api.fetchActors>>);
    await vi.waitFor(() => {
      expect(api.fetchActors).toHaveBeenCalledTimes(2);
      expect(api.fetchActors).toHaveBeenNthCalledWith(2, "g-demo", true);
    });
  });

  it("queued unread refresh is not downgraded after a readonly request finishes", async () => {
    let resolvePureRead: ((value: Awaited<ReturnType<typeof api.fetchActors>>) => void) | null = null;
    vi.mocked(api.fetchActors).mockImplementation((groupId: string, includeUnread = false) => {
      if (includeUnread) {
        return Promise.resolve({
          ok: true,
          result: { actors: [{ id: "peer-1", unread_count: 5 }] },
        }) as ReturnType<typeof api.fetchActors>;
      }
      return new Promise((resolve) => {
        resolvePureRead = resolve as (value: Awaited<ReturnType<typeof api.fetchActors>>) => void;
      }) as ReturnType<typeof api.fetchActors>;
    });

    const readonlyRefresh = useGroupStore.getState().refreshActors("g-demo", { includeUnread: false });
    const queuedUnreadRefresh = useGroupStore.getState().refreshActors("g-demo", { includeUnread: true });

    expect(api.fetchActors).toHaveBeenCalledTimes(1);
    expect(api.fetchActors).toHaveBeenNthCalledWith(1, "g-demo", false);

    resolvePureRead?.({ ok: true, result: { actors: [{ id: "peer-1" }] } } as Awaited<ReturnType<typeof api.fetchActors>>);
    await readonlyRefresh;
    await queuedUnreadRefresh;

    await vi.waitFor(() => {
      expect(api.fetchActors).toHaveBeenCalledTimes(2);
      expect(api.fetchActors).toHaveBeenNthCalledWith(2, "g-demo", true);
    });
  });

  it("warmGroup preserves unread counts in the prefetched actor snapshot", async () => {
    const warmGroupId = "g-warm";
    useGroupStore.setState({
      selectedGroupId: "g-current",
      groups: [
        { group_id: "g-current", title: "Current", topic: "", state: "active" },
        { group_id: warmGroupId, title: "Warm", topic: "", state: "active" },
      ],
      groupOrder: ["g-current", warmGroupId],
    });

    await useGroupStore.getState().warmGroup(warmGroupId);
    expect(api.fetchActors).toHaveBeenCalledWith(warmGroupId, false);
  });

  it("refreshPresentation updates the selected group snapshot", async () => {
    vi.mocked(api.fetchPresentation).mockResolvedValue({
      ok: true,
      result: {
        group_id: "g-demo",
        presentation: {
          v: 1,
          updated_at: "2026-03-21T00:00:00Z",
          highlight_slot_id: "slot-2",
          slots: [
            { slot_id: "slot-1", index: 1, card: null },
            { slot_id: "slot-2", index: 2, card: { slot_id: "slot-2", title: "Deck", card_type: "markdown", published_by: "peer-1", published_at: "2026-03-21T00:00:00Z", content: { mode: "inline", markdown: "# deck" } } },
          ],
        },
      },
    } as Awaited<ReturnType<typeof api.fetchPresentation>>);

    await useGroupStore.getState().refreshPresentation("g-demo");

    expect(api.fetchPresentation).toHaveBeenCalledWith("g-demo");
    expect(useGroupStore.getState().groupPresentation?.highlight_slot_id).toBe("slot-2");
    expect(useGroupStore.getState().groupPresentation?.slots[1]?.card?.title).toBe("Deck");
  });

  it("refreshPresentation ignores stale responses that resolve after a newer local update", async () => {
    let resolveRefresh: ((value: Awaited<ReturnType<typeof api.fetchPresentation>>) => void) | null = null;
    vi.mocked(api.fetchPresentation).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRefresh = resolve as (value: Awaited<ReturnType<typeof api.fetchPresentation>>) => void;
        }) as ReturnType<typeof api.fetchPresentation>
    );

    const refreshPromise = useGroupStore.getState().refreshPresentation("g-demo");
    useGroupStore.getState().setGroupPresentation({
      v: 1,
      updated_at: "2026-03-25T14:00:00Z",
      highlight_slot_id: "slot-3",
      slots: [],
    });

    resolveRefresh?.({
      ok: true,
      result: {
        group_id: "g-demo",
        presentation: {
          v: 1,
          updated_at: "2026-03-25T13:59:00Z",
          highlight_slot_id: "slot-1",
          slots: [],
        },
      },
    } as Awaited<ReturnType<typeof api.fetchPresentation>>);

    await refreshPromise;

    expect(useGroupStore.getState().groupPresentation?.highlight_slot_id).toBe("slot-3");
  });

  it("refreshSettings ignores stale responses that resolve after a newer local update", async () => {
    let resolveRefresh: ((value: Awaited<ReturnType<typeof api.fetchSettings>>) => void) | null = null;
    vi.mocked(api.fetchSettings).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRefresh = resolve as (value: Awaited<ReturnType<typeof api.fetchSettings>>) => void;
        }) as ReturnType<typeof api.fetchSettings>
    );

    const refreshPromise = useGroupStore.getState().refreshSettings("g-demo");
    useGroupStore.getState().setGroupSettings({
      auto_accept: true,
    } as never);

    resolveRefresh?.({
      ok: true,
      result: {
        settings: {
          auto_accept: false,
        },
      },
    } as Awaited<ReturnType<typeof api.fetchSettings>>);

    await refreshPromise;

    expect((useGroupStore.getState().groupSettings as { auto_accept?: boolean } | null)?.auto_accept).toBe(true);
  });

  it("loadGroup falls back to full fresh context when summary snapshot is missing and no cached context exists", async () => {
    vi.mocked(api.fetchContext)
      .mockResolvedValueOnce({
        ok: true,
        result: {
          version: "ctxv:1",
          coordination: { tasks: [] },
          agent_states: [],
          meta: { summary_snapshot: { state: "missing" } },
        },
      } as Awaited<ReturnType<typeof api.fetchContext>>)
      .mockResolvedValueOnce({
        ok: true,
        result: {
          version: "ctxv:2",
          coordination: { tasks: [{ id: "t-1", title: "Real", outcome: "x" }] },
          agent_states: [{ id: "peer-1" }],
          meta: {},
        },
      } as Awaited<ReturnType<typeof api.fetchContext>>);

    await useGroupStore.getState().loadGroup("g-demo");

    await vi.waitFor(() => {
      expect(api.fetchContext).toHaveBeenNthCalledWith(1, "g-demo", { detail: "summary" });
      expect(api.fetchContext).toHaveBeenNthCalledWith(2, "g-demo", { detail: "full", fresh: true });
      expect(useGroupStore.getState().groupContext?.version).toBe("ctxv:2");
      expect(useGroupStore.getState().groupContext?.agent_states?.[0]?.id).toBe("peer-1");
    });
  });

  it("loadGroup preserves existing context when summary snapshot is stale", async () => {
    useGroupStore.setState({
      groupDoc: {
        group_id: "g-demo",
        title: "Demo",
        topic: "",
        state: "active",
      },
      groupContext: {
        version: "ctxv:cached",
        coordination: { tasks: [{ id: "t-cached", title: "Cached", outcome: "y" }] },
        agent_states: [{ id: "peer-cached" }],
      },
    });
    vi.mocked(api.fetchContext).mockResolvedValue({
      ok: true,
      result: {
        version: "ctxv:stale",
        coordination: { tasks: [] },
        agent_states: [],
        meta: { summary_snapshot: { state: "stale" } },
      },
    } as Awaited<ReturnType<typeof api.fetchContext>>);

    await useGroupStore.getState().loadGroup("g-demo");

    await vi.waitFor(() => {
      expect(api.fetchContext).toHaveBeenCalledTimes(1);
      expect(api.fetchContext).toHaveBeenNthCalledWith(1, "g-demo", { detail: "summary" });
      expect(useGroupStore.getState().groupContext?.version).toBe("ctxv:cached");
      expect(useGroupStore.getState().groupContext?.agent_states?.[0]?.id).toBe("peer-cached");
    });
  });

  it("openChatWindow ignores stale window responses from an older request", async () => {
    let resolveFirstWindow: ((value: Awaited<ReturnType<typeof api.fetchMessageWindow>>) => void) | null = null;
    let resolveSecondWindow: ((value: Awaited<ReturnType<typeof api.fetchMessageWindow>>) => void) | null = null;

    vi.mocked(api.fetchMessageWindow)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirstWindow = resolve as (value: Awaited<ReturnType<typeof api.fetchMessageWindow>>) => void;
          }) as ReturnType<typeof api.fetchMessageWindow>
      )
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveSecondWindow = resolve as (value: Awaited<ReturnType<typeof api.fetchMessageWindow>>) => void;
          }) as ReturnType<typeof api.fetchMessageWindow>
      );

    const firstPromise = useGroupStore.getState().openChatWindow("g-demo", "msg-old");
    const secondPromise = useGroupStore.getState().openChatWindow("g-demo", "msg-new");

    resolveSecondWindow?.({
      ok: true,
      result: {
        center_id: "msg-new",
        center_index: 0,
        events: [
          {
            id: "msg-new",
            kind: "chat.message",
            ts: "2026-03-25T09:02:00Z",
            by: "peer-1",
            data: { text: "new" },
          },
        ],
        has_more_before: true,
        has_more_after: false,
      },
    } as Awaited<ReturnType<typeof api.fetchMessageWindow>>);

    resolveFirstWindow?.({
      ok: true,
      result: {
        center_id: "msg-old",
        center_index: 0,
        events: [
          {
            id: "msg-old",
            kind: "chat.message",
            ts: "2026-03-25T09:00:00Z",
            by: "peer-1",
            data: { text: "old" },
          },
        ],
        has_more_before: false,
        has_more_after: true,
      },
    } as Awaited<ReturnType<typeof api.fetchMessageWindow>>);

    await Promise.all([firstPromise, secondPromise]);

    const bucket = useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket?.chatWindow?.centerEventId).toBe("msg-new");
    expect(bucket?.chatWindow?.events.map((event) => event.id)).toEqual(["msg-new"]);
    expect(bucket?.isChatWindowLoading).toBe(false);
  });

  it("loadGroup merges tail data with messages appended before tail settles", async () => {
    let resolveTail: ((value: Awaited<ReturnType<typeof api.fetchLedgerTail>>) => void) | null = null;
    vi.mocked(api.fetchLedgerTail).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveTail = resolve as (value: Awaited<ReturnType<typeof api.fetchLedgerTail>>) => void;
        }) as ReturnType<typeof api.fetchLedgerTail>
    );

    const loadPromise = useGroupStore.getState().loadGroup("g-demo");
    useGroupStore.getState().appendEvent(
      {
        id: "msg-live",
        kind: "chat.message",
        ts: "2026-03-25T09:01:00Z",
        by: "peer-1",
        data: { text: "live" },
      },
      "g-demo"
    );

    resolveTail?.({
      ok: true,
      result: {
        events: [
          {
            id: "msg-old",
            kind: "chat.message",
            ts: "2026-03-25T09:00:00Z",
            by: "peer-1",
            data: { text: "old" },
          },
        ],
        has_more: false,
        count: 1,
      },
    } as Awaited<ReturnType<typeof api.fetchLedgerTail>>);

    await loadPromise;

    await vi.waitFor(() => {
      const bucket = useGroupStore.getState().chatByGroup["g-demo"];
      expect(bucket?.events.map((event) => event.id)).toEqual(["msg-old", "msg-live"]);
      expect(bucket?.hasLoadedTail).toBe(true);
      expect(bucket?.hasMoreHistory).toBe(false);
    });

    expect(api.fetchLedgerStatuses).toHaveBeenCalledWith("g-demo", ["msg-old", "msg-live"]);
  });

  it("restores inactive-group messages from cache after the bucket is rebuilt", async () => {
    useGroupStore.setState({
      groups: [
        { group_id: "g-demo", title: "Demo", topic: "", state: "active" },
        { group_id: "g-other", title: "Other", topic: "", state: "active" },
      ],
      groupOrder: ["g-demo", "g-other"],
      selectedGroupId: "g-demo",
      chatByGroup: {
        "g-demo": {
          events: [
            {
              id: "msg-old",
              kind: "chat.message",
              ts: "2026-03-25T09:00:00Z",
              by: "peer-1",
              data: { text: "old" },
            },
          ],
          chatWindow: null,
          hasMoreHistory: false,
          hasLoadedTail: true,
          isLoadingHistory: false,
          isChatWindowLoading: false,
        },
      },
    });

    useGroupStore.getState().setSelectedGroupId("g-other");
    useGroupStore.getState().appendEvent(
      {
        id: "msg-late",
        kind: "chat.message",
        ts: "2026-03-25T09:01:00Z",
        by: "user",
        group_id: "g-demo",
        data: { text: "late" },
      },
      "g-demo"
    );

    useGroupStore.setState((state) => ({
      chatByGroup: {
        "g-other": state.chatByGroup["g-other"],
      },
    }));

    useGroupStore.getState().setSelectedGroupId("g-demo");

    const restoredBucket = useGroupStore.getState().chatByGroup["g-demo"];
    expect(restoredBucket?.events.map((event) => event.id)).toEqual(["msg-old", "msg-late"]);
  });

  it("loadMoreHistory closes history pagination when the older page is empty", async () => {
    useGroupStore.setState({
      selectedGroupId: "g-demo",
      chatByGroup: {
        "g-demo": {
          events: [
            {
              id: "msg-2",
              kind: "chat.message",
              ts: "2026-03-25T09:00:00Z",
              by: "peer-1",
              data: { text: "latest" },
            },
          ],
          chatWindow: null,
          hasMoreHistory: true,
          hasLoadedTail: true,
          isLoadingHistory: false,
          isChatWindowLoading: false,
        },
      },
    });
    vi.mocked(api.fetchOlderMessages).mockResolvedValueOnce({
      ok: true,
      result: { events: [], has_more: true, count: 0 },
    } as Awaited<ReturnType<typeof api.fetchOlderMessages>>);

    await useGroupStore.getState().loadMoreHistory("g-demo");

    const bucket = useGroupStore.getState().chatByGroup["g-demo"];
    expect(api.fetchOlderMessages).toHaveBeenCalledWith("g-demo", "msg-2", 50);
    expect(bucket?.isLoadingHistory).toBe(false);
    expect(bucket?.hasMoreHistory).toBe(false);
  });

  it("loadMoreHistory closes history pagination when the older page only contains duplicates", async () => {
    useGroupStore.setState({
      selectedGroupId: "g-demo",
      chatByGroup: {
        "g-demo": {
          events: [
            {
              id: "msg-1",
              kind: "chat.message",
              ts: "2026-03-25T08:59:00Z",
              by: "peer-1",
              data: { text: "old" },
            },
            {
              id: "msg-2",
              kind: "chat.message",
              ts: "2026-03-25T09:00:00Z",
              by: "peer-1",
              data: { text: "latest" },
            },
          ],
          chatWindow: null,
          hasMoreHistory: true,
          hasLoadedTail: true,
          isLoadingHistory: false,
          isChatWindowLoading: false,
        },
      },
    });
    vi.mocked(api.fetchOlderMessages).mockResolvedValueOnce({
      ok: true,
      result: {
        events: [
          {
            id: "msg-1",
            kind: "chat.message",
            ts: "2026-03-25T08:59:00Z",
            by: "peer-1",
            data: { text: "old" },
          },
        ],
        has_more: true,
        count: 1,
      },
    } as Awaited<ReturnType<typeof api.fetchOlderMessages>>);

    await useGroupStore.getState().loadMoreHistory("g-demo");

    const bucket = useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket?.events.map((event) => event.id)).toEqual(["msg-1", "msg-2"]);
    expect(bucket?.isLoadingHistory).toBe(false);
    expect(bucket?.hasMoreHistory).toBe(false);
  });
});

describe("useGroupStore streaming placeholder cleanup", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    localStorageMock.clear();
    const mod = await importFreshStore();
    mod.useGroupStore.setState({
      groups: [{ group_id: "g-demo", title: "Demo", topic: "", state: "active" }],
      groupOrder: ["g-demo"],
      archivedGroupIds: [],
      selectedGroupId: "g-demo",
      chatByGroup: {
        "g-demo": {
          events: [],
          streamingEvents: [],
          streamingTextByStreamId: {},
          streamingActivitiesByStreamId: {},
          chatWindow: null,
          hasMoreHistory: false,
          hasLoadedTail: true,
          isLoadingHistory: false,
          isChatWindowLoading: false,
        },
      },
      groupDoc: null,
      events: [],
      chatWindow: null,
      actors: [],
      groupContext: null,
      groupSettings: null,
      groupPresentation: null,
      runtimes: [],
      hasMoreHistory: false,
      isLoadingHistory: false,
      isChatWindowLoading: false,
    });
  });

  it("clearEmptyStreamingEventsForActor removes unanchored placeholder-only entries and cached activity state", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "pending:e1:peer-1",
      ts: "2026-04-03T15:53:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:e1:peer-1",
        pending_placeholder: true,
        activities: [{ id: "a1", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T15:53:00Z" }],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingActivities("pending:e1:peer-1", [
      { id: "a1", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T15:53:00Z" },
    ], "g-demo");

    mod.useGroupStore.getState().clearEmptyStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toEqual([]);
    expect(bucket.streamingActivitiesByStreamId).toEqual({});
    expect(bucket.streamingTextByStreamId).toEqual({});
  });

  it("clearEmptyStreamingEventsForActor preserves pending-bound queued placeholders before canonical reply arrives", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "pending:e1:peer-1",
      ts: "2026-04-03T15:53:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:e1:peer-1",
        pending_event_id: "e1",
        pending_placeholder: true,
        activities: [{ id: "a1", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T15:53:00Z" }],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingActivities("pending:e1:peer-1", [
      { id: "a1", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T15:53:00Z" },
    ], "g-demo");

    mod.useGroupStore.getState().clearEmptyStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.pending_event_id).toBe("e1");
    expect(bucket.streamingActivitiesByStreamId["pending:e1:peer-1"]).toEqual([
      { id: "a1", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T15:53:00Z" },
    ]);
  });

  it("clearEmptyStreamingEventsForActor preserves completed streaming text entries", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "stream:s1",
      ts: "2026-04-03T15:53:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: false,
      data: {
        text: "final answer",
        to: ["user"],
        stream_id: "s1",
        activities: [],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingText("s1", "final answer", "g-demo");

    mod.useGroupStore.getState().clearEmptyStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingTextByStreamId["s1"]).toBe("final answer");
  });

  it("clearTransientStreamingEventsForActor removes commentary-only transient streams", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "stream:c1",
      ts: "2026-04-03T16:12:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: false,
      data: {
        text: "Inspecting stream wiring",
        to: ["user"],
        stream_id: "c1",
        transient_stream: true,
        stream_phase: "commentary",
        activities: [],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingText("c1", "Inspecting stream wiring", "g-demo");

    mod.useGroupStore.getState().clearTransientStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toEqual([]);
    expect(bucket.streamingTextByStreamId).toEqual({});
    expect(bucket.streamingActivitiesByStreamId).toEqual({});
  });

  it("clearTransientStreamingEventsForActor keeps the current transient bubble until a non-transient stream takes over", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "stream:c2",
      ts: "2026-04-03T16:12:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "Inspecting stream wiring",
        to: ["user"],
        stream_id: "c2",
        pending_event_id: "evt-2",
        transient_stream: true,
        stream_phase: "commentary",
        activities: [],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingText("c2", "Inspecting stream wiring", "g-demo");

    mod.useGroupStore.getState().clearTransientStreamingEventsForActor("peer-1", "g-demo");

    let bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("c2");

    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "stream:f2",
      ts: "2026-04-03T16:12:01Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "Final answer",
        to: ["user"],
        stream_id: "f2",
        pending_event_id: "evt-2",
        transient_stream: false,
        stream_phase: "final_answer",
        activities: [],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingText("f2", "Final answer", "g-demo");

    mod.useGroupStore.getState().clearTransientStreamingEventsForActor("peer-1", "g-demo");

    bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("f2");
    expect(bucket.streamingTextByStreamId["c2"]).toBeUndefined();
    expect(bucket.streamingTextByStreamId["f2"]).toBe("Final answer");
    expect(bucket.replySessionsByPendingEventId["evt-2"]).toMatchObject({
      pendingEventId: "evt-2",
      actorId: "peer-1",
      currentStreamId: "f2",
      phase: "streaming",
    });
  });

  it("headless preview indexes stay session-scoped instead of falling back to canonical chat messages", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().appendEvent({
      id: "evt-visible",
      ts: "2026-04-03T16:15:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      data: {
        text: "visible reply",
        to: ["user"],
        stream_id: "stream-visible",
      },
    }, "g-demo");

    let bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.latestActorTextByActorId["peer-1"]).toBeUndefined();
    expect(bucket.latestActorActivitiesByActorId["peer-1"]).toBeUndefined();

    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "stream:s3",
      ts: "2026-04-03T16:15:01Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: false,
      data: {
        text: "session reply",
        to: ["user"],
        stream_id: "s3",
        activities: [],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingText("s3", "session reply", "g-demo");

    bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.latestActorTextByActorId["peer-1"]).toBe("session reply");

    mod.useGroupStore.getState().clearStreamingEventsForActor("peer-1", "g-demo");

    bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.latestActorTextByActorId["peer-1"]).toBeUndefined();
    expect(bucket.latestActorActivitiesByActorId["peer-1"]).toBeUndefined();
  });

  it("keeps recent headless preview sessions per actor instead of only the latest one", async () => {
    const mod = await importFreshStore();

    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "evt-older",
      streamId: "stream-older",
      ts: "2026-04-09T10:00:00Z",
      fullText: "Older answer",
      eventText: "Older answer",
      activities: [{ id: "activity-older", kind: "thinking", status: "completed", summary: "Reviewed prior output", ts: "2026-04-09T09:59:59Z" }],
      completed: true,
      transientStream: false,
      phase: "final_answer",
      groupId: "g-demo",
    });

    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "evt-newer",
      streamId: "stream-newer",
      ts: "2026-04-09T10:05:00Z",
      fullText: "Newer answer",
      eventText: "Newer answer",
      activities: [{ id: "activity-newer", kind: "tool", status: "started", summary: "Opened reducer file", ts: "2026-04-09T10:04:58Z" }],
      completed: false,
      transientStream: false,
      phase: "final_answer",
      groupId: "g-demo",
    });

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.previewSessionsByActorId["peer-1"]?.map((session) => session.pendingEventId)).toEqual([
      "evt-older",
      "evt-newer",
    ]);
    expect(bucket.latestActorPreviewByActorId["peer-1"]?.pendingEventId).toBe("evt-newer");
    expect(bucket.latestActorTextByActorId["peer-1"]).toBe("Newer answer");
  });

  it("preserves activity history in state beyond the old twelve-item limit", async () => {
    const mod = await importFreshStore();
    const activities = Array.from({ length: 14 }, (_, index) => ({
      id: `activity-${index + 1}`,
      kind: "tool",
      status: "completed",
      summary: `step ${index + 1}`,
      ts: `2026-04-09T10:${String(index).padStart(2, "0")}:00Z`,
    }));

    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "evt-many-activities",
      streamId: "stream-many-activities",
      ts: "2026-04-09T10:14:30Z",
      fullText: "Final answer with full trace",
      eventText: "Final answer with full trace",
      activities,
      completed: false,
      transientStream: false,
      phase: "final_answer",
      groupId: "g-demo",
    });

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingActivitiesByStreamId["stream-many-activities"]).toHaveLength(14);
    expect(bucket.previewSessionsByActorId["peer-1"]?.[0]?.activities).toHaveLength(14);
  });

  it("clearEmptyStreamingEventsForActor preserves non-queued process bubbles", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "pending:e-process:peer-1",
      ts: "2026-04-03T16:20:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:e-process:peer-1",
        pending_event_id: "e-process",
        pending_placeholder: true,
        activities: [{ id: "a-process", kind: "command", status: "completed", summary: "RUN sed -n '1,260p' src/foo.ts", ts: "2026-04-03T16:20:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().clearEmptyStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.activities).toEqual([
      { id: "a-process", kind: "command", status: "completed", summary: "RUN sed -n '1,260p' src/foo.ts", ts: "2026-04-03T16:20:00Z" },
    ]);
  });

  it("clearEmptyStreamingEventsForActor preserves non-queued process bubbles after canonical reply arrives", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().appendEvent({
      id: "evt-process-final",
      ts: "2026-04-03T16:20:01Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      data: {
        text: "最终答复",
        reply_to: "e-process",
        stream_id: "stream-process-final",
        to: ["user"],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "pending:e-process:peer-1",
      ts: "2026-04-03T16:20:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: false,
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:e-process:peer-1",
        pending_event_id: "e-process",
        pending_placeholder: false,
        activities: [{ id: "a-process", kind: "command", status: "completed", summary: "RUN sed -n '1,260p' src/foo.ts", ts: "2026-04-03T16:20:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().clearEmptyStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("pending:e-process:peer-1");
    expect(bucket.streamingEvents[0]?.data?.activities).toEqual([
      { id: "a-process", kind: "command", status: "completed", summary: "RUN sed -n '1,260p' src/foo.ts", ts: "2026-04-03T16:20:00Z" },
    ]);
  });

  it("completeStreamingEventsForActor turns process bubbles into stable non-streaming entries", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "pending:e3:peer-1",
      ts: "2026-04-03T16:21:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:e3:peer-1",
        pending_event_id: "e3",
        pending_placeholder: true,
        activities: [{ id: "a3", kind: "command", status: "completed", summary: "RUN rg -n foo", ts: "2026-04-03T16:21:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().completeStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?._streaming).toBe(false);
    expect(bucket.streamingEvents[0]?.data?.pending_placeholder).toBe(false);
  });

  it("completeStreamingEventsForActor keeps fresh local queued placeholders streaming", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "local:msg-1:peer-1",
      ts: "2026-04-03T16:22:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "local:msg-1:peer-1",
        pending_event_id: "local_123",
        pending_placeholder: true,
        activities: [{ id: "queued:local-1", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T16:22:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().completeStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?._streaming).toBe(true);
    expect(bucket.streamingEvents[0]?.data?.pending_placeholder).toBe(true);
  });

  it("promoteStreamingEventToStream upgrades pending placeholder in place", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "pending:e2:peer-1",
      ts: "2026-04-03T16:10:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:e2:peer-1",
        pending_event_id: "e2",
        pending_placeholder: true,
        activities: [{ id: "a2", kind: "tool", status: "started", summary: "reading files", ts: "2026-04-03T16:10:00Z" }],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingActivities("pending:e2:peer-1", [
      { id: "a2", kind: "tool", status: "started", summary: "reading files", ts: "2026-04-03T16:10:00Z" },
    ], "g-demo");

    mod.useGroupStore.getState().promoteStreamingEventToStream("peer-1", "e2", "stream-2", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("stream-2");
    expect(bucket.streamingEvents[0]?.data?.pending_placeholder).toBe(false);
    expect(bucket.streamingActivitiesByStreamId["stream-2"]).toEqual([
      { id: "a2", kind: "tool", status: "started", summary: "reading files", ts: "2026-04-03T16:10:00Z" },
    ]);
    expect(bucket.streamingActivitiesByStreamId["pending:e2:peer-1"]).toBeUndefined();
  });

  it("reconcileStreamingMessage upgrades a pending placeholder to the final stream in one record", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "pending:e4:peer-1",
      ts: "2026-04-03T16:25:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:e4:peer-1",
        pending_event_id: "e4",
        pending_placeholder: true,
        activities: [{ id: "a4", kind: "thinking", status: "started", summary: "thinking", ts: "2026-04-03T16:25:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "e4",
      streamId: "stream-4",
      ts: "2026-04-03T16:25:02Z",
      fullText: "final answer",
      eventText: "final answer",
      activities: [{ id: "a4", kind: "thinking", status: "completed", summary: "thinking", ts: "2026-04-03T16:25:01Z" }],
      completed: false,
      transientStream: false,
      phase: "final_answer",
      groupId: "g-demo",
    });

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("stream-4");
    expect(bucket.streamingEvents[0]?.data?.pending_placeholder).toBe(false);
    expect(bucket.streamingTextByStreamId["stream-4"]).toBe("final answer");
    expect(bucket.streamingTextByStreamId["pending:e4:peer-1"]).toBeUndefined();
    expect(bucket.streamingActivitiesByStreamId["stream-4"]).toEqual([
      { id: "a4", kind: "thinking", status: "completed", summary: "thinking", ts: "2026-04-03T16:25:01Z" },
    ]);
    expect(bucket.pendingEventIdByStreamId["stream-4"]).toBe("e4");
    expect(bucket.replySessionsByPendingEventId["e4"]).toMatchObject({
      pendingEventId: "e4",
      actorId: "peer-1",
      currentStreamId: "stream-4",
      phase: "streaming",
    });
  });

  it("promoteStreamingEventToStream does not rebind an existing real transcript stream", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "evt-phase",
      streamId: "stream-commentary-phase",
      ts: "2026-04-09T10:00:00Z",
      fullText: "Inspecting the implementation",
      eventText: "Inspecting the implementation",
      activities: [],
      completed: true,
      transientStream: true,
      phase: "commentary",
      groupId: "g-demo",
    });

    mod.useGroupStore.getState().promoteStreamingEventToStream("peer-1", "evt-phase", "stream-final-phase", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("stream-commentary-phase");
    expect(bucket.streamingTextByStreamId["stream-commentary-phase"]).toBe("Inspecting the implementation");
    expect(bucket.streamingTextByStreamId["stream-final-phase"]).toBeUndefined();
  });

  it("reconcileStreamingMessage keeps separate real transcript streams for the same pending event", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "evt-multi-stream",
      streamId: "stream-commentary-multi",
      ts: "2026-04-09T10:00:00Z",
      fullText: "Reviewing the store path",
      eventText: "Reviewing the store path",
      activities: [],
      completed: true,
      transientStream: true,
      phase: "commentary",
      groupId: "g-demo",
    });

    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "evt-multi-stream",
      streamId: "stream-final-multi",
      ts: "2026-04-09T10:00:02Z",
      fullText: "Final answer body",
      eventText: "Final answer body",
      activities: [],
      completed: false,
      transientStream: false,
      phase: "final_answer",
      groupId: "g-demo",
    });

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents.map((event) => event.data?.stream_id)).toEqual([
      "stream-commentary-multi",
      "stream-final-multi",
    ]);
    expect(bucket.streamingTextByStreamId["stream-commentary-multi"]).toBe("Reviewing the store path");
    expect(bucket.streamingTextByStreamId["stream-final-multi"]).toBe("Final answer body");
    expect(bucket.latestActorPreviewByActorId["peer-1"]?.transcriptBlocks.map((block) => block.streamPhase)).toEqual([
      "commentary",
      "final_answer",
    ]);
    expect(bucket.latestActorPreviewByActorId["peer-1"]?.transcriptBlocks.map((block) => block.text)).toEqual([
      "Reviewing the store path",
      "Final answer body",
    ]);
  });

  it("promoteStreamingEventToStream binds the latest local queued placeholder when server pending ids differ", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "local:msg-1:peer-1",
      ts: "2026-04-03T16:26:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "local:msg-1:peer-1",
        pending_event_id: "local_123",
        pending_placeholder: true,
        activities: [{ id: "queued:local-1", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T16:26:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().promoteStreamingEventToStream("peer-1", "server-e5", "stream-5", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.id).toBe("local:msg-1:peer-1");
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("stream-5");
    expect(bucket.streamingEvents[0]?.data?.pending_event_id).toBe("server-e5");
    expect(bucket.streamingEvents[0]?.data?.pending_placeholder).toBe(false);
  });

  it("reconcileStreamingMessage reuses the latest local queued placeholder when server pending ids differ", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "local:msg-2:peer-1",
      ts: "2026-04-03T16:27:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "local:msg-2:peer-1",
        pending_event_id: "local_456",
        pending_placeholder: true,
        activities: [{ id: "queued:local-2", kind: "queued", status: "started", summary: "queued", ts: "2026-04-03T16:27:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "server-e6",
      streamId: "stream-6",
      ts: "2026-04-03T16:27:02Z",
      fullText: "hello world",
      eventText: "hello world",
      activities: [],
      completed: false,
      transientStream: false,
      phase: "final_answer",
      groupId: "g-demo",
    });

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(1);
    expect(bucket.streamingEvents[0]?.id).toBe("local:msg-2:peer-1");
    expect(bucket.streamingEvents[0]?.data?.stream_id).toBe("stream-6");
    expect(bucket.streamingEvents[0]?.data?.pending_event_id).toBe("server-e6");
    expect(bucket.streamingEvents[0]?.data?.text).toBe("hello world");
    expect(bucket.streamingTextByStreamId["stream-6"]).toBe("hello world");
    expect(bucket.streamingTextByStreamId["local:msg-2:peer-1"]).toBeUndefined();
  });

  it("preserves commentary transcript blocks after final answer starts and transient commentary is cleared", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "stream:commentary-7",
      ts: "2026-04-09T10:00:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "stream-commentary-7",
        pending_event_id: "evt-7",
        pending_placeholder: false,
        stream_phase: "commentary",
        transient_stream: true,
        activities: [],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingText("stream-commentary-7", "Investigating the race condition", "g-demo");

    mod.useGroupStore.getState().reconcileStreamingMessage({
      actorId: "peer-1",
      pendingEventId: "evt-7",
      streamId: "stream-final-7",
      ts: "2026-04-09T10:00:02Z",
      fullText: "Final answer body",
      eventText: "Final answer body",
      activities: [],
      completed: false,
      transientStream: false,
      phase: "final_answer",
      groupId: "g-demo",
    });

    mod.useGroupStore.getState().completeStreamingEventsForActor("peer-1", "g-demo");
    mod.useGroupStore.getState().clearTransientStreamingEventsForActor("peer-1", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.latestActorPreviewByActorId["peer-1"]).toMatchObject({
      actorId: "peer-1",
      pendingEventId: "evt-7",
      currentStreamId: "stream-final-7",
      latestText: "Final answer body",
    });
    expect(bucket.latestActorPreviewByActorId["peer-1"]?.transcriptBlocks.map((block) => block.streamPhase)).toEqual([
      "commentary",
      "final_answer",
    ]);
    expect(bucket.latestActorPreviewByActorId["peer-1"]?.transcriptBlocks.map((block) => block.text)).toEqual([
      "Investigating the race condition",
      "Final answer body",
    ]);
  });

  it("upsertStreamingActivity keeps streamless process activity off commentary text streams", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "stream:commentary-1",
      ts: "2026-04-04T14:39:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "我已经在真实 UI 里抓到现象了。",
        to: ["user"],
        stream_id: "commentary-1",
        pending_event_id: "evt-1",
        pending_placeholder: false,
        stream_phase: "commentary",
        transient_stream: true,
        activities: [],
      },
    }, "g-demo");
    mod.useGroupStore.getState().upsertStreamingText("commentary-1", "我已经在真实 UI 里抓到现象了。", "g-demo");

    mod.useGroupStore.getState().upsertStreamingActivity("peer-1", { pendingEventId: "evt-1" }, {
      id: "tool-1",
      kind: "tool",
      status: "started",
      summary: "chrome-devtools:wait_for",
      ts: "2026-04-04T14:39:01Z",
    }, "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.streamingEvents).toHaveLength(2);
    expect(bucket.streamingEvents.find((event) => event.data?.stream_id === "commentary-1")?.data?.text).toBe("我已经在真实 UI 里抓到现象了。");
    expect(bucket.streamingEvents.find((event) => event.data?.stream_id === "commentary-1")?.data?.activities).toEqual([]);
    expect(bucket.streamingEvents.find((event) => event.data?.stream_id === "pending:evt-1:peer-1")?.data?.activities).toEqual([
      { id: "tool-1", kind: "tool", status: "started", summary: "chrome-devtools:wait_for", ts: "2026-04-04T14:39:01Z" },
    ]);
    expect(bucket.replySessionsByPendingEventId["evt-1"]).toMatchObject({
      pendingEventId: "evt-1",
      actorId: "peer-1",
      currentStreamId: "pending:evt-1:peer-1",
      phase: "pending",
    });
  });

  it("promoteStreamingEventsByPrefix migrates local placeholder session to canonical pending event", async () => {
    const mod = await importFreshStore();
    mod.useGroupStore.getState().upsertStreamingEvent({
      id: "local:msg-3:peer-1",
      ts: "2026-04-04T15:00:00Z",
      kind: "chat.message",
      group_id: "g-demo",
      by: "peer-1",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "local:msg-3:peer-1",
        pending_event_id: "msg-3",
        pending_placeholder: true,
        activities: [{ id: "queued:local-3", kind: "queued", status: "started", summary: "queued", ts: "2026-04-04T15:00:00Z" }],
      },
    }, "g-demo");

    mod.useGroupStore.getState().promoteStreamingEventsByPrefix("local:msg-3:", "evt-3", "g-demo");

    const bucket = mod.useGroupStore.getState().chatByGroup["g-demo"];
    expect(bucket.replySessionsByPendingEventId["msg-3"]).toBeUndefined();
    expect(bucket.replySessionsByPendingEventId["evt-3"]).toMatchObject({
      pendingEventId: "evt-3",
      actorId: "peer-1",
      currentStreamId: "pending:evt-3:peer-1",
    });
    expect(bucket.pendingEventIdByStreamId["local:msg-3:peer-1"]).toBeUndefined();
    expect(bucket.pendingEventIdByStreamId["pending:evt-3:peer-1"]).toBe("evt-3");
  });
});
