// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../../src/services/api";
import { useGroupStore } from "../../src/stores/useGroupStore";

vi.mock("../../src/services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../src/services/api")>()),
  fetchGroups: vi.fn(),
}));

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  useGroupStore.setState({
    groups: [],
    groupOrder: ["removed", "c", "b", "a"],
    archivedGroupIds: [],
    selectedGroupId: "",
    groupDoc: null,
    actors: [],
    chatByGroup: {},
  });
  vi.mocked(api.fetchGroups).mockResolvedValue({
    ok: true,
    result: {
      groups: [
        { group_id: "a", title: "A", state: "idle", topic: "" },
        { group_id: "b", title: "B", state: "idle", topic: "" },
        { group_id: "c", title: "C", state: "idle", topic: "" },
      ],
    },
  } as Awaited<ReturnType<typeof api.fetchGroups>>);
});

it("opens the first unarchived Group in sidebar order when the remembered Group is gone", async () => {
  localStorage.setItem("cccc-selected-group-id", "removed");
  useGroupStore.setState({ selectedGroupId: "removed", archivedGroupIds: ["c"] });

  await useGroupStore.getState().refreshGroups();

  expect(useGroupStore.getState().selectedGroupId).toBe("b");
  expect(localStorage.getItem("cccc-selected-group-id")).toBe("b");
  expect(useGroupStore.getState().groupOrder).toEqual(["c", "b", "a"]);
});

it("uses sidebar order when all Groups are archived", async () => {
  useGroupStore.setState({ archivedGroupIds: ["a", "b", "c"] });

  await useGroupStore.getState().refreshGroups();

  expect(useGroupStore.getState().selectedGroupId).toBe("c");
});

it("keeps a valid remembered Group even if it is archived", async () => {
  localStorage.setItem("cccc-selected-group-id", "a");
  useGroupStore.setState({ archivedGroupIds: ["a"] });

  await useGroupStore.getState().refreshGroups();

  expect(useGroupStore.getState().selectedGroupId).toBe("a");
});

it("does not move an active selection when sidebar order or archiving changes", async () => {
  useGroupStore.setState({ selectedGroupId: "a", archivedGroupIds: ["a"] });

  await useGroupStore.getState().refreshGroups();

  expect(useGroupStore.getState().selectedGroupId).toBe("a");
});
