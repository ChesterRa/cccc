import { expect, it } from "vite-plus/test";
import type { Task } from "../../types";
import {
  directSubtasks,
  nextChecklistItemId,
  priorityLabel,
  subtaskProgressByParent,
  taskStatusLabel,
  taskToDraft,
} from "./model";

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

it("gives a new checklist item an id no item has", () => {
  const items = (...ids: string[]) => ids.map((id) => ({ id, text: id }));
  expect(nextChecklistItemId([])).toBe("item-1");
  expect(nextChecklistItemId(items("item-1", "item-3", "custom"))).toBe("item-4");
  // A number past what is held exactly: the lowest free one, not a loop.
  expect(nextChecklistItemId(items("item-9007199254740992", "item-1"))).toBe("item-2");
  expect(nextChecklistItemId(items("item-9007199254740991", "item-1", "item-2"))).toBe("item-3");
});

it("gives each draft checklist item its own id, the same each time", () => {
  const stored: Task = {
    id: "T1",
    checklist: [
      { id: "", text: "A" },
      { id: "item-1", text: "B" },
      { id: "item-1", text: "C" },
    ],
  };
  const ids = taskToDraft(stored).checklist.map((item) => item.id);
  expect(new Set(ids).size).toBe(3);
  expect(ids[1]).toBe("item-1");
  expect(taskToDraft(stored).checklist.map((item) => item.id)).toEqual(ids);
});

it("translates known priorities while keeping custom values intact", () => {
  const tr = (key: string) => `translated:${key}`;
  expect(priorityLabel(" Medium ", tr)).toBe("translated:context.priorityMedium");
  expect(priorityLabel("night shift", tr)).toBe("night shift");
});

it("labels only its own values, not what every object has", () => {
  const tr = (key: string) => `translated:${key}`;
  for (const value of ["__proto__", "constructor", "toString"]) {
    expect(priorityLabel(value, tr)).toBe(value);
    expect(taskStatusLabel(value, tr)).toBe(value);
  }
  expect(taskStatusLabel("done", tr)).toBe("translated:context.done");
});
