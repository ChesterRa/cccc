import { describe, expect, it } from "vite-plus/test";
import type { Task } from "../../../src/types";
import {
  applyTaskPage,
  createTaskColumnPages,
  removeTaskDetail,
  shouldRefreshTaskRevision,
  upsertTaskDetail,
} from "../../../src/features/contextModal/taskBoardData";
import { deriveTaskBoardView } from "../../../src/features/contextModal/taskBoardView";

const task = (id: string, status: string = "planned"): Task => ({ id, title: id, status });

describe("task board paging model", () => {
  it("refreshes only toward a newer canonical task revision", () => {
    expect(shouldRefreshTaskRevision("tasksv:3", "tasksv:4")).toBe(true);
    expect(shouldRefreshTaskRevision("tasksv:4", "tasksv:3")).toBe(false);
    expect(shouldRefreshTaskRevision("opaque-a", "opaque-b")).toBe(true);
  });

  it("appends pages without duplicating overlapping tasks", () => {
    const first = applyTaskPage(
      createTaskColumnPages(),
      "planned",
      {
        tasks: [task("T003"), task("T002")],
        count: 2,
        totalCount: 3,
        offset: 0,
        limit: 2,
        hasMore: true,
        tasksVersion: "v1",
        facets: {
          statusCounts: { planned: 3 },
          blocked: 0,
          waitingUser: 0,
          pendingHandoffs: 0,
          unassigned: 3,
          assignees: [],
        },
      },
      false,
    );
    const next = applyTaskPage(
      first,
      "planned",
      {
        tasks: [task("T002"), task("T001")],
        count: 2,
        totalCount: 3,
        offset: 2,
        limit: 2,
        hasMore: false,
        tasksVersion: "v1",
        facets: {
          statusCounts: { planned: 3 },
          blocked: 0,
          waitingUser: 0,
          pendingHandoffs: 0,
          unassigned: 3,
          assignees: [],
        },
      },
      true,
    );

    expect(next.planned.items.map((item) => item.id)).toEqual(["T003", "T002", "T001"]);
    expect(next.planned.totalCount).toBe(3);
    expect(next.planned.nextOffset).toBe(4);
    expect(next.planned.hasMore).toBe(false);
  });

  it("keeps an exact deep-link task without inflating the server total", () => {
    const columns = createTaskColumnPages();
    columns.done = {
      items: [task("T010", "done")],
      totalCount: 40,
      nextOffset: 30,
      hasMore: true,
      loading: false,
    };

    const next = upsertTaskDetail(columns, task("T001", "done"));

    expect(next.done.items.map((item) => item.id)).toEqual(["T010", "T001"]);
    expect(next.done.totalCount).toBe(40);
    expect(next.done.nextOffset).toBe(30);
  });

  it("removes a missing exact task without changing the server cursor", () => {
    const columns = createTaskColumnPages();
    columns.planned = {
      items: [task("T010"), task("T999")],
      totalCount: 40,
      nextOffset: 30,
      hasMore: true,
      loading: false,
    };

    const next = removeTaskDetail(columns, "T999");

    expect(next.planned.items.map((item) => item.id)).toEqual(["T010"]);
    expect(next.planned.totalCount).toBe(40);
    expect(next.planned.nextOffset).toBe(30);
  });

  it("derives lifecycle totals without counting archived tasks as live work", () => {
    const view = deriveTaskBoardView(
      createTaskColumnPages(),
      {},
      {
        statusCounts: { planned: 2, active: 3, done: 5, archived: 7 },
        blocked: 1,
        waitingUser: 0,
        pendingHandoffs: 0,
        unassigned: 2,
        assignees: ["peer"],
      },
    );

    expect(view.tasksSummary).toEqual({ total: 10, planned: 2, active: 3, done: 5, archived: 7 });
  });
});
