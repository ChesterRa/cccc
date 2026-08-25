import { describe, expect, it } from "vite-plus/test";

import type { Task } from "../../types";
import { createTaskColumnPages, type TaskColumnPages } from "./taskBoardData";
import { retainTaskDuringReload } from "./useExactTaskRetention";

function withPlannedTask(task: Task): TaskColumnPages {
  const columns = createTaskColumnPages();
  columns.planned = { ...columns.planned, items: [task], totalCount: 1 };
  return columns;
}

describe("retainTaskDuringReload", () => {
  it("keeps the selected card while refreshed columns are loading", () => {
    const selected = { id: "T001", title: "unsaved editor source", status: "planned" };
    const current = withPlannedTask(selected);
    const loading = createTaskColumnPages();
    loading.planned.loading = true;

    const retained = retainTaskDuringReload(current, loading, selected.id);

    expect(retained.planned.items).toEqual([selected]);
    expect(retained.planned.loading).toBe(true);
  });

  it("keeps the selected card when a refreshed page omits it", () => {
    const selected = { id: "T001", title: "unsaved editor source", status: "planned" };
    const current = withPlannedTask(selected);

    const retained = retainTaskDuringReload(current, createTaskColumnPages(), selected.id);

    expect(retained.planned.items).toEqual([selected]);
  });

  it("prefers a refreshed card over the retained copy", () => {
    const stale = { id: "T001", title: "old title", status: "planned" };
    const refreshed = { ...stale, title: "new title" };

    const retained = retainTaskDuringReload(
      withPlannedTask(stale),
      withPlannedTask(refreshed),
      stale.id,
    );

    expect(retained.planned.items).toEqual([refreshed]);
  });
});
