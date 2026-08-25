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

let batchVersion = "v1";

function page(status: TaskPageStatus, offset = 0, tasksVersion = batchVersion) {
  const count = status === "active" ? 2 : 30;
  const total = status === "active" ? 2 : status === "archived" ? 40 : 60;
  return {
    tasks: Array.from({ length: count }, (_, index) => ({
      id: `${status}-${offset + index + 1}`,
      title: `${status} ${offset + index + 1}`,
      status,
    })),
    count,
    totalCount: total,
    offset,
    limit: 30,
    hasMore: offset + count < total,
    tasksVersion,
    facets: {
      statusCounts: { planned: 60, active: 2, done: 60, archived: 40 },
      blocked: 1,
      waitingUser: 0,
      pendingHandoffs: 0,
      unassigned: 3,
      assignees: ["peer"],
    },
  };
}

function pages(statuses: readonly TaskPageStatus[], tasksVersion = batchVersion) {
  const result = Object.fromEntries(
    statuses.map((status) => [status, page(status, 0, tasksVersion)]),
  );
  return {
    ok: true as const,
    result: {
      pages: result,
      tasksVersion,
      facets: page("planned", 0, tasksVersion).facets,
      taskIndex: [{ id: "T900", title: "Index-only task", status: "active" }],
    },
  };
}

function Probe({
  includeArchived = false,
  contextVersion = "v1",
  selectedTaskId = "",
}: {
  includeArchived?: boolean;
  contextVersion?: string;
  selectedTaskId?: string;
}) {
  const data = useTaskBoardData({
    groupId: "g-1",
    isOpen: true,
    query: "",
    assignee: "__all__",
    filter: "all",
    includeArchived,
    contextTasksVersion: contextVersion,
    selectedTaskId,
  });
  return (
    <div>
      <span data-testid="planned">{data.columns.planned.items.length}</span>
      <span data-testid="archived">{data.columns.archived.items.length}</span>
      <span data-testid="index">{data.taskIndex.length}</span>
      <button type="button" data-testid="detail" onClick={() => void data.loadTaskDetail("T999")} />
      <button type="button" data-testid="more" onClick={() => void data.loadMore("planned")} />
      <button
        type="button"
        data-testid="archive-more"
        onClick={() => void data.loadMore("archived")}
      />
    </div>
  );
}

describe("useTaskBoardData", () => {
  beforeEach(() => {
    batchVersion = "v1";
    apiMocks.fetchTaskPage.mockReset();
    apiMocks.fetchTaskPages.mockReset();
    apiMocks.fetchTaskById.mockReset();
    apiMocks.fetchTaskPages.mockImplementation(
      (_groupId: string, options: { statuses: readonly TaskPageStatus[] }) =>
        Promise.resolve(pages(options.statuses)),
    );
    apiMocks.fetchTaskPage.mockImplementation(
      (_groupId: string, options: { status: TaskPageStatus; offset?: number }) =>
        Promise.resolve({ ok: true, result: page(options.status, options.offset || 0) }),
    );
    apiMocks.fetchTaskById.mockResolvedValue({
      ok: true,
      result: {
        task: { id: "T999", title: "Deep task", status: "planned" },
        tasksVersion: "v1",
        deleteInfo: { allowed: true, total: 1, reason: "" },
      },
    });
  });

  it("loads live columns in one scan and atomically includes archive on expansion", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(1));
    expect(apiMocks.fetchTaskPages.mock.calls[0]?.[1]).toMatchObject({
      statuses: ["planned", "active", "done"],
      includeIndex: true,
    });
    expect(apiMocks.fetchTaskPage).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="index"]')?.textContent).toBe("1");

    await act(async () => root.render(<Probe includeArchived contextVersion="" />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(2));
    expect(apiMocks.fetchTaskPages.mock.calls[1]?.[1].statuses).toEqual([
      "planned",
      "active",
      "done",
      "archived",
    ]);
    expect(host.querySelector('[data-testid="archived"]')?.textContent).toBe("30");
    await act(async () => root.unmount());
  });

  it("keeps the server cursor after inserting a deep-link detail", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe />));
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("30"),
    );
    await act(async () => host.querySelector<HTMLElement>('[data-testid="detail"]')?.click());
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("31"),
    );

    await act(async () => host.querySelector<HTMLElement>('[data-testid="more"]')?.click());
    await vi.waitFor(() => expect(apiMocks.fetchTaskPage).toHaveBeenCalledTimes(1));
    expect(apiMocks.fetchTaskPage).toHaveBeenCalledWith(
      "g-1",
      expect.objectContaining({ status: "planned", offset: 30, limit: 30 }),
    );
    await act(async () => root.unmount());
  });

  it("refreshes pages and the selected detail when the context task revision changes", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe contextVersion="v1" selectedTaskId="T999" />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(1));

    batchVersion = "v2";
    await act(async () => root.render(<Probe contextVersion="v2" selectedTaskId="T999" />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(apiMocks.fetchTaskById).toHaveBeenCalledWith("g-1", "T999"));
    await act(async () => root.unmount());
  });

  it("discards an archive continuation from another revision and reloads atomically", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe includeArchived contextVersion="" />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(1));
    batchVersion = "v2";

    await act(async () => host.querySelector<HTMLElement>('[data-testid="archive-more"]')?.click());
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(2));
    expect(host.querySelector('[data-testid="archived"]')?.textContent).toBe("30");
    await act(async () => root.unmount());
  });
});
