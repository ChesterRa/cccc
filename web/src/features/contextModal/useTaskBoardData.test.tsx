// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../../services/api";
import type { TaskPageResult, TaskPagesResult, TaskPageStatus } from "../../services/api";
import { useTaskBoardData } from "./useTaskBoardData";
import type { UseTaskBoardDataOptions } from "./taskBoardData";

vi.mock("../../services/api", () => ({
  fetchTaskPage: vi.fn(),
  fetchTaskPages: vi.fn(),
  fetchTaskById: vi.fn(),
}));

type PagesResponse = Awaited<ReturnType<typeof api.fetchTaskPages>>;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function pages(counts: Partial<Record<TaskPageStatus, number>>): PagesResponse {
  const facets: TaskPagesResult["facets"] = {
    statusCounts: {},
    blocked: 0,
    waitingUser: 0,
    pendingHandoffs: 0,
    unassigned: 0,
    assignees: [],
  };
  const result: TaskPagesResult = { pages: {}, tasksVersion: "tasksv:1", facets, taskIndex: [] };
  for (const status of ["planned", "active", "done", "archived"] as const) {
    result.pages[status] = {
      tasks: [],
      count: 0,
      totalCount: counts[status] || 0,
      totalCountKnown: true,
      offset: 0,
      limit: 30,
      hasMore: false,
      tasksVersion: result.tasksVersion,
      facets,
    } satisfies TaskPageResult;
  }
  return { ok: true, result };
}

let board: ReturnType<typeof useTaskBoardData> | null = null;
function Probe({ options }: { options: UseTaskBoardDataOptions }) {
  board = useTaskBoardData(options);
  return <div>{board.matchingCount ?? "unknown"}</div>;
}

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  vi.mocked(api.fetchTaskPages).mockReset();
  vi.mocked(api.fetchTaskById).mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  board = null;
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

it("shows only counts for the currently completed query, without retained tasks inflating them", async () => {
  const next = deferred<PagesResponse>();
  const archived = deferred<PagesResponse>();
  vi.mocked(api.fetchTaskPages)
    .mockResolvedValueOnce(pages({ planned: 1, done: 2 }))
    .mockReturnValueOnce(next.promise)
    .mockReturnValueOnce(archived.promise);
  const options: UseTaskBoardDataOptions = {
    groupId: "g-test",
    isOpen: true,
    query: "needle",
    assignee: "__all__",
    filter: "all",
    includeArchived: false,
    selectedTaskId: "T99",
  };
  const render = async (changes: Partial<UseTaskBoardDataOptions> = {}) =>
    act(async () => root.render(<Probe options={{ ...options, ...changes }} />));

  await render();
  expect(board?.matchingCount).toBe(3);
  await render({ query: "new query" });
  // The old result is hidden immediately, before the 250ms search debounce.
  expect(board?.matchingCount).toBeNull();
  await act(async () => vi.advanceTimersByTimeAsync(250));
  expect(board?.matchingCount).toBeNull();
  await act(async () => next.resolve(pages({})));
  expect(board?.matchingCount).toBe(0);

  vi.mocked(api.fetchTaskById).mockResolvedValueOnce({
    ok: true,
    result: {
      task: { id: "T99", title: "Open but not a match", status: "active" },
      tasksVersion: "tasksv:1",
      deleteInfo: { allowed: true, total: 1, reason: "" },
    },
  });
  await act(async () => {
    await board?.loadTaskDetail("T99");
  });
  expect(board?.columns.active.totalCount).toBe(1);
  expect(board?.matchingCount).toBe(0);

  await render({ query: "new query", includeArchived: true });
  expect(board?.matchingCount).toBeNull();
  await act(async () => archived.resolve(pages({ active: 2, archived: 4 })));
  expect(board?.matchingCount).toBe(6);
});

it("does not present an old count after a new request fails or the group changes", async () => {
  vi.mocked(api.fetchTaskPages)
    .mockResolvedValueOnce(pages({ active: 2 }))
    .mockResolvedValueOnce({ ok: false, error: { code: "NETWORK_ERROR", message: "offline" } });
  const options: UseTaskBoardDataOptions = {
    groupId: "g-one",
    isOpen: true,
    query: "",
    assignee: "__all__",
    filter: "blocked",
    includeArchived: false,
  };
  await act(async () => root.render(<Probe options={options} />));
  expect(board?.matchingCount).toBe(2);
  await act(async () => root.render(<Probe options={{ ...options, groupId: "g-two" }} />));
  expect(board?.error).toBe("offline");
  expect(board?.matchingCount).toBeNull();
});

it("does not present a precise count if a page's count was not supplied", async () => {
  const response = pages({ active: 2 });
  if (!response.ok || !response.result.pages.done) throw new Error("test fixture");
  response.result.pages.done.totalCountKnown = false;
  vi.mocked(api.fetchTaskPages).mockResolvedValueOnce(response);
  await act(async () =>
    root.render(
      <Probe
        options={{
          groupId: "g-one",
          isOpen: true,
          query: "test",
          assignee: "__all__",
          filter: "all",
          includeArchived: false,
        }}
      />,
    ),
  );
  expect(board?.error).toBe("");
  expect(board?.matchingCount).toBeNull();
});

it("does not attribute a filtered response for the old debounced search to new input", async () => {
  const oldQueryWithFilter = deferred<PagesResponse>();
  const newQueryWithFilter = deferred<PagesResponse>();
  vi.mocked(api.fetchTaskPages)
    .mockResolvedValueOnce(pages({ planned: 1 }))
    .mockReturnValueOnce(oldQueryWithFilter.promise)
    .mockReturnValueOnce(newQueryWithFilter.promise);
  const options: UseTaskBoardDataOptions = {
    groupId: "g-one",
    isOpen: true,
    query: "old",
    assignee: "__all__",
    filter: "all",
    includeArchived: false,
  };
  await act(async () => root.render(<Probe options={options} />));
  expect(board?.matchingCount).toBe(1);
  await act(async () =>
    root.render(<Probe options={{ ...options, query: "new", filter: "blocked" }} />),
  );
  await act(async () => oldQueryWithFilter.resolve(pages({ planned: 2 })));
  expect(board?.matchingCount).toBeNull();
  await act(async () => vi.advanceTimersByTimeAsync(250));
  await act(async () => newQueryWithFilter.resolve(pages({ active: 4 })));
  expect(board?.matchingCount).toBe(4);
});
