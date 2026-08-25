// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const apiMocks = vi.hoisted(() => ({
  fetchTaskPage: vi.fn(),
  fetchTaskPages: vi.fn(),
  fetchTaskById: vi.fn(),
}));

vi.mock("../../../src/services/api", () => apiMocks);

import { useTaskBoardData } from "../../../src/features/contextModal/useTaskBoardData";

const facets = {
  statusCounts: { planned: 1, active: 0, done: 0, archived: 0 },
  blocked: 0,
  waitingUser: 0,
  pendingHandoffs: 0,
  unassigned: 0,
  assignees: [],
};

function successfulPages() {
  const page = {
    tasks: [{ id: "planned-1", title: "Listed task", status: "planned" }],
    count: 1,
    totalCount: 1,
    offset: 0,
    limit: 30,
    hasMore: false,
    tasksVersion: "tasksv:1",
    facets,
  };
  return {
    ok: true as const,
    result: {
      pages: {
        planned: page,
        active: { ...page, tasks: [], count: 0 },
        done: { ...page, tasks: [], count: 0 },
      },
      tasksVersion: "tasksv:1",
      facets,
      taskIndex: [],
    },
  };
}

function Probe({ groupId }: { groupId: string }) {
  const data = useTaskBoardData({
    groupId,
    isOpen: true,
    query: "",
    assignee: "__all__",
    filter: "all",
    includeArchived: false,
    contextTasksVersion: "tasksv:1",
    selectedTaskId: "T999",
  });
  return (
    <div>
      <span data-testid="planned">{data.columns.planned.items.length}</span>
      <span data-testid="error">{data.error}</span>
      <span data-testid="details">{Object.keys(data.deleteInfoById).length}</span>
      <button type="button" data-testid="detail" onClick={() => void data.loadTaskDetail("T999")} />
    </div>
  );
}

describe("useTaskBoardData group switching", () => {
  beforeEach(() => {
    apiMocks.fetchTaskPage.mockReset();
    apiMocks.fetchTaskPages.mockReset();
    apiMocks.fetchTaskById.mockReset();
  });

  it("clears the previous group's selected card when the new group load fails", async () => {
    apiMocks.fetchTaskPages
      .mockResolvedValueOnce(successfulPages())
      .mockResolvedValueOnce({
        ok: false,
        error: { code: "unavailable", message: "new group unavailable" },
      });
    apiMocks.fetchTaskById.mockResolvedValue({
      ok: true,
      result: {
        task: { id: "T999", title: "Old group task", status: "planned" },
        tasksVersion: "tasksv:1",
        deleteInfo: { allowed: true, total: 1, reason: "" },
      },
    });
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe groupId="g-1" />));
    await vi.waitFor(() => expect(apiMocks.fetchTaskPages).toHaveBeenCalledTimes(1));
    await act(async () => host.querySelector<HTMLElement>('[data-testid="detail"]')?.click());
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("2"),
    );

    await act(async () => root.render(<Probe groupId="g-2" />));

    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="error"]')?.textContent).toBe(
        "new group unavailable",
      ),
    );
    expect(host.querySelector('[data-testid="planned"]')?.textContent).toBe("0");
    expect(host.querySelector('[data-testid="details"]')?.textContent).toBe("0");
    await act(async () => root.unmount());
  });
});
