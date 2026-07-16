import { describe, expect, it } from "vite-plus/test";

import type { Task, TaskMessageRef } from "../types";
import { getTaskRefChipLabel, getTaskRefStateKey } from "./taskRefs";

function makeRef(overrides: Partial<TaskMessageRef> = {}): TaskMessageRef {
  return {
    kind: "task_ref",
    task_id: "T001",
    title: "Investigate routing bug",
    status: "planned",
    ...overrides,
  };
}

function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "T001",
    title: "Investigate routing bug",
    status: "planned",
    waiting_on: "none",
    blocked_by: [],
    ...overrides,
  };
}

describe("taskRefs", () => {
  it("uses live task truth when deriving chip label", () => {
    const ref = makeRef({ title: "Old title" });
    const task = makeTask({ title: "Fresh title from task" });
    expect(getTaskRefChipLabel(ref, task)).toBe("T001 · Fresh title from task");
  });

  it("projects waiting_user and handoff from live task", () => {
    const ref = makeRef();
    expect(getTaskRefStateKey(ref, makeTask({ waiting_on: "user" }))).toBe("waiting_user");
    expect(getTaskRefStateKey(ref, makeTask({ handoff_to: "reviewer" }))).toBe("handoff");
  });

  it("falls back to ref state when live task is unavailable", () => {
    const ref = makeRef({ status: "done" });
    expect(getTaskRefStateKey(ref, null)).toBe("done");
  });
});
