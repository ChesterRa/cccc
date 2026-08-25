// @vitest-environment happy-dom

import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import {
  fetchTaskById,
  fetchTaskPage,
  fetchTaskPages,
  fetchTasksByIds,
} from "../../src/services/api/tasks";

const jsonResponse = (result: Record<string, unknown>) =>
  new Response(JSON.stringify({ ok: true, result }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });

describe("tasks api", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("sends paging and server-side filter parameters", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        jsonResponse({
          tasks: [{ id: "T009", title: "Paged", status: "active" }],
          count: 1,
          total_count: 31,
          offset: 30,
          limit: 30,
          has_more: false,
          tasks_version: "v2",
          facets: {
            status_counts: { active: 31 },
            blocked: 2,
            waiting_user: 1,
            pending_handoffs: 3,
            unassigned: 4,
            assignees: ["peer"],
          },
        }),
      );
    vi.stubGlobal("fetch", fetchMock);

    const response = await fetchTaskPage("g 1", {
      status: "active",
      offset: 30,
      query: "needle",
      assignee: "peer",
      attention: "blocked",
    });

    expect(response.ok).toBe(true);
    const url = new URL(String(fetchMock.mock.calls[0]?.[0]), "http://localhost");
    expect(url.pathname).toBe("/api/v1/groups/g%201/tasks");
    expect(Object.fromEntries(url.searchParams)).toMatchObject({
      status: "active",
      offset: "30",
      limit: "30",
      query: "needle",
      assignee: "peer",
      attention: "blocked",
    });
    if (response.ok) {
      expect(response.result.totalCount).toBe(31);
      expect(response.result.facets.statusCounts.active).toBe(31);
    }
  });

  it("normalizes atomic multi-column pages and the unfiltered task index", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        pages: {
          planned: {
            tasks: [{ id: "T002", title: "Visible", status: "planned" }],
            count: 1,
            total_count: 1,
            offset: 0,
            limit: 30,
            has_more: false,
          },
          active: { tasks: [], count: 0, total_count: 0, offset: 0, limit: 30 },
        },
        tasks_version: "tasksv:4",
        facets: { status_counts: { planned: 1 } },
        task_index: [
          { id: "T002", title: "Visible", status: "planned" },
          { id: "T001", title: "Filtered out", status: "done" },
        ],
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    const response = await fetchTaskPages("g1", {
      statuses: ["planned", "active"],
      query: "Visible",
      includeIndex: true,
    });

    const url = new URL(String(fetchMock.mock.calls[0]?.[0]), "http://localhost");
    expect(url.searchParams.get("statuses")).toBe("planned,active");
    expect(url.searchParams.get("include_index")).toBe("true");
    expect(response.ok && response.result.pages.planned?.tasks[0]?.id).toBe("T002");
    expect(response.ok && response.result.taskIndex.map((task) => task.id)).toEqual([
      "T002",
      "T001",
    ]);
  });

  it("fetches referenced tasks in one ordered batch", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse({
            tasks: [{ id: "T003", title: "Third", status: "done" }],
            tasks_version: "tasksv:5",
          }),
        ),
    );

    const response = await fetchTasksByIds("g1", ["T003", "T001"]);

    expect(response.ok && response.result.tasks[0]?.status).toBe("done");
    expect(String(vi.mocked(fetch).mock.calls[0]?.[0])).toContain("task_ids=T003%2CT001");
  });

  it("normalizes exact-task delete metadata", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse({
            task: { id: "T001", title: "Exact", status: "planned" },
            tasks_version: "v3",
            delete_info: { allowed: true, total: 2, reason: "" },
          }),
        ),
    );

    const response = await fetchTaskById("g1", "T001");

    expect(response.ok && response.result.task.id).toBe("T001");
    expect(response.ok && response.result.deleteInfo).toEqual({
      allowed: true,
      total: 2,
      reason: "",
    });
  });
});
