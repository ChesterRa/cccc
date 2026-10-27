// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { Task } from "../../../types";
import { createContextModalUi } from "../ui";
import { TaskCardBadges } from "./TaskCardBadges";
import { TaskSubtaskList } from "./TaskSubtaskList";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const tr = (_key: string, fallback: string, options?: Record<string, unknown>) =>
  fallback.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => String(options?.[name] ?? ""));

function rendered(node: ReactNode, check: (host: HTMLElement) => void) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(node));
    check(host);
  } finally {
    act(() => root.unmount());
    host.remove();
  }
}

it("lists the open task's subtasks with their progress and opens the chosen one", () => {
  const subtasks: Task[] = [
    { id: "T2", title: "Collect evidence", status: "done" },
    { id: "T3", title: "Draft report" },
  ];
  const onOpenTask = vi.fn();
  rendered(
    <TaskSubtaskList
      tr={tr}
      ui={createContextModalUi(false)}
      subtasks={subtasks}
      onOpenTask={onOpenTask}
    />,
    (host) => {
      expect(host.textContent).toContain("1 of 2 done");
      const rows = [...host.querySelectorAll("button")];
      expect(rows.map((row) => row.textContent)).toEqual([
        "T2Collect evidencedone",
        "T3Draft reportplanned",
      ]);
      act(() => rows[1].click());
      expect(onOpenTask).toHaveBeenCalledWith("T3");
    },
  );
  rendered(
    <TaskSubtaskList
      tr={tr}
      ui={createContextModalUi(false)}
      subtasks={[]}
      onOpenTask={onOpenTask}
    />,
    (host) => expect(host.textContent).toBe(""),
  );
});

it("shows subtask progress on a card only when live subtasks exist", () => {
  const parent: Task = { id: "T1", title: "Release", assignee: "lead" };
  rendered(<TaskCardBadges task={parent} tr={tr} subtasks={{ done: 2, total: 3 }} />, (host) =>
    expect(host.textContent).toContain("Subtasks 2/3"),
  );
  for (const subtasks of [undefined, { done: 0, total: 0 }]) {
    rendered(<TaskCardBadges task={parent} tr={tr} subtasks={subtasks} />, (host) =>
      expect(host.textContent).not.toContain("Subtasks"),
    );
  }
});
