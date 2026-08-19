// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { Task } from "../../../src/types";
import { createContextModalUi } from "../../../src/components/ContextModal/ui";

const dndMocks = vi.hoisted(() => ({ useDraggable: vi.fn(), useDroppable: vi.fn() }));

vi.mock("@dnd-kit/core", () => ({
  useDraggable: dndMocks.useDraggable,
  useDroppable: dndMocks.useDroppable,
}));

import {
  TASK_COLUMN_PAGE_SIZE,
  TaskBoardColumn,
} from "../../../src/components/ContextModal/coordination/TaskBoardColumn";

const tr = (_key: string, fallback: string, options?: Record<string, unknown>) =>
  fallback.replace("{{count}}", String(options?.count ?? ""));

function tasks(count: number): Task[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `task-${index + 1}`,
    title: `Task ${index + 1}`,
    status: "planned",
    updated_at: `2026-08-19T10:${String(index).padStart(2, "0")}:00Z`,
  }));
}

describe("TaskBoardColumn", () => {
  beforeEach(() => {
    dndMocks.useDraggable.mockImplementation(() => ({
      attributes: {},
      listeners: {},
      setNodeRef: vi.fn(),
      transform: null,
      isDragging: false,
    }));
    dndMocks.useDroppable.mockImplementation(() => ({ setNodeRef: vi.fn(), isOver: false }));
  });

  it("registers only the first page of draggable cards until the user asks for more", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <TaskBoardColumn
          columnKey="planned"
          label="Planned"
          items={tasks(75)}
          tr={tr}
          ui={createContextModalUi(false)}
          syncBusy={false}
          selectedTaskId=""
          onSelectTask={vi.fn()}
          onMoveTaskToStatus={vi.fn()}
        />,
      );
    });

    expect(host.querySelectorAll("[data-task-id]")).toHaveLength(TASK_COLUMN_PAGE_SIZE);
    expect(dndMocks.useDraggable).toHaveBeenCalledTimes(TASK_COLUMN_PAGE_SIZE);
    const more = Array.from(host.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("Show 30 more"),
    );
    expect(more).toBeDefined();

    await act(async () => more?.click());
    expect(host.querySelectorAll("[data-task-id]")).toHaveLength(TASK_COLUMN_PAGE_SIZE * 2);

    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps a directly selected task mounted even when it is outside the first page", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <TaskBoardColumn
          columnKey="planned"
          label="Planned"
          items={tasks(75)}
          tr={tr}
          ui={createContextModalUi(false)}
          syncBusy={false}
          selectedTaskId="task-65"
          onSelectTask={vi.fn()}
          onMoveTaskToStatus={vi.fn()}
        />,
      );
    });

    expect(host.querySelector('[data-task-id="task-65"]')).not.toBeNull();
    expect(host.querySelectorAll("[data-task-id]")).toHaveLength(65);

    await act(async () => root.unmount());
    host.remove();
  });
});
