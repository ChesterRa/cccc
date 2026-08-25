// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { TaskPageStatus } from "../../../src/services/api/tasks";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const apiMocks = vi.hoisted(() => ({
  fetchTaskPage: vi.fn(),
  fetchTaskPages: vi.fn(),
  fetchTaskById: vi.fn(),
}));

vi.mock("../../../src/services/api", () => apiMocks);

import { useTaskBoardData } from "../../../src/features/contextModal/useTaskBoardData";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function page(status: TaskPageStatus) {
  return {
    tasks: Array.from({ length: 30 }, (_, index) => ({
      id: `${status}-${index + 1}`,
      title: `${status} ${index + 1}`,
      status,
    })),
    count: 30,
    totalCount: 60,
    offset: 0,
    limit: 30,
    hasMore: true,
    tasksVersion: "tasksv:1",
    facets: {
      statusCounts: { planned: 60, active: 60, done: 60, archived: 60 },
      blocked: 0,
      waitingUser: 0,
      pendingHandoffs: 0,
      unassigned: 0,
      assignees: [],
    },
  };
}

function pages(statuses: readonly TaskPageStatus[]) {
  return {
    ok: true as const,
    result: {
      pages: Object.fromEntries(statuses.map((status) => [status, page(status)])),
      tasksVersion: "tasksv:1",
      facets: page("planned").facets,
      taskIndex: [],
    },
  };
}

function Probe({
  includeArchived = false,
  selectedTaskId = "T999",
}: {
  includeArchived?: boolean;
  selectedTaskId?: string;
}) {
  const data = useTaskBoardData({
    groupId: "g-1",
    isOpen: true,
    query: "",
    assignee: "__all__",
    filter: "all",
    includeArchived,
    contextTasksVersion: "tasksv:1",
    selectedTaskId,
  });
  return (
    <div>
      <span data-testid="planned">{data.columns.planned.items.length}</span>
      <span data-testid="archived">{data.columns.archived.items.length}</span>
      <button
        type="button"
        data-testid="detail"
        onClick={() => void data.loadTaskDetail(selectedTaskId)}
      />
    </div>
  );
}

describe("useTaskBoardData exact-task races", () => {
  beforeEach(() => {
    apiMocks.fetchTaskPage.mockReset();
    apiMocks.fetchTaskPages.mockReset();
    apiMocks.fetchTaskById.mockReset();
  });

  it("keeps a deep-link card when its exact response wins the initial-page race", async () => {
    const batch = deferred<ReturnType<typeof pages>>();
    apiMocks.fetchTaskPages.mockReturnValue(batch.promise);
    apiMocks.fetchTaskById.mockResolvedValue({
      ok: true,
      result: {
        task: { id: "T999", title: "Deep task", status: "planned" },
        tasksVersion: "tasksv:1",
        deleteInfo: { allowed: true, total: 1, reason: "" },
      },
    });
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(1));
    await act(async () => host.querySelector<HTMLElement>('[data-testid="detail"]')?.click());
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("1"),
    );

    await act(async () => batch.resolve(pages(["planned", "active", "done"])));

    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("31"),
    );
    await act(async () => root.unmount());
  });

  it("keeps an archived deep-link card through the archive batch reload", async () => {
    const archiveBatch = deferred<ReturnType<typeof pages>>();
    apiMocks.fetchTaskPages
      .mockResolvedValueOnce(pages(["planned", "active", "done"]))
      .mockReturnValueOnce(archiveBatch.promise);
    apiMocks.fetchTaskById.mockResolvedValue({
      ok: true,
      result: {
        task: { id: "T999", title: "Archived deep task", status: "archived" },
        tasksVersion: "tasksv:1",
        deleteInfo: { allowed: true, total: 1, reason: "" },
      },
    });
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(1));
    await act(async () => host.querySelector<HTMLElement>('[data-testid="detail"]')?.click());
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="archived"]')?.textContent).toBe("1"),
    );

    await act(async () => root.render(<Probe includeArchived />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(2));
    await act(async () => archiveBatch.resolve(pages(["planned", "active", "done", "archived"])));

    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="archived"]')?.textContent).toBe("31"),
    );
    await act(async () => root.unmount());
  });

  it("does not carry a previously opened detail into a later filtered page", async () => {
    apiMocks.fetchTaskPages.mockResolvedValue(pages(["planned", "active", "done"]));
    apiMocks.fetchTaskById.mockResolvedValue({
      ok: true,
      result: {
        task: { id: "T999", title: "Previously opened", status: "planned" },
        tasksVersion: "tasksv:1",
        deleteInfo: { allowed: true, total: 1, reason: "" },
      },
    });
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(1));
    await act(async () => host.querySelector<HTMLElement>('[data-testid="detail"]')?.click());
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("31"),
    );

    await act(async () => root.render(<Probe includeArchived selectedTaskId="" />));

    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(2));
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("30"),
    );
    await act(async () => root.unmount());
  });

  it("keeps the selected card when a page response arrives before its exact detail", async () => {
    const exact = deferred<Awaited<ReturnType<typeof apiMocks.fetchTaskById>>>();
    const refreshed = pages(["planned", "active", "done", "archived"]);
    refreshed.result.pages.planned = { ...page("planned"), tasks: [], count: 0 };
    apiMocks.fetchTaskPages
      .mockResolvedValueOnce(pages(["planned", "active", "done"]))
      .mockResolvedValueOnce(refreshed);
    apiMocks.fetchTaskById.mockReturnValue(exact.promise);
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe selectedTaskId="planned-1" />));
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("30"),
    );
    await act(async () => host.querySelector<HTMLElement>('[data-testid="detail"]')?.click());
    await act(async () => root.render(<Probe includeArchived selectedTaskId="planned-1" />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(2));
    expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("1");
    await act(async () => root.unmount());
  });
});
