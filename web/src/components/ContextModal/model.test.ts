import { expect, it } from "vite-plus/test";
import type { Task } from "../../types";
import { directSubtasks, subtaskProgressByParent } from "./model";

const task = (id: string, parentId: string | null, status?: Task["status"]): Task => ({
  id,
  title: id,
  parent_id: parentId,
  status,
});

const tasks: Task[] = [
  task("T1", null, "active"),
  task("T2", "T1", "done"),
  task("T10", "T1", "active"),
  task("T3", "T1"),
  task("T4", "T1", "archived"),
  task("T5", "T2", "done"),
  task("T6", "T6", "done"),
];

it("counts direct live subtasks and treats only done as done", () => {
  const progress = subtaskProgressByParent(tasks);
  // T3 has no status (planned), T4 is archived, and grandchild T5 counts for T2.
  expect(progress.get("T1")).toEqual({ done: 1, total: 3 });
  expect(progress.get("T2")).toEqual({ done: 1, total: 1 });
  expect(progress.has("T6")).toBe(false);
  expect(progress.has("T10")).toBe(false);
});

it("lists the same direct subtasks in task-number order", () => {
  expect(directSubtasks("T1", tasks).map((item) => item.id)).toEqual(["T2", "T3", "T10"]);
  expect(directSubtasks("T6", tasks)).toEqual([]);
  expect(directSubtasks("", tasks)).toEqual([]);
});
