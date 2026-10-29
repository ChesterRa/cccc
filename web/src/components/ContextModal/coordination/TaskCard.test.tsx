// @vitest-environment happy-dom
import { DndContext } from "@dnd-kit/core";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { createContextModalUi } from "../ui";
import { TaskCard } from "./TaskCard";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("uses separate operable controls for opening and moving a task", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const task = { id: "T1", title: "Review the detailed plan", status: "planned" };
  const open = vi.fn();
  const move = vi.fn();
  const tr = (_key: string, fallback: string, values?: Record<string, unknown>) =>
    fallback.replace(/\{\{(\w+)\}\}/g, (_match, key: string) => String(values?.[key] ?? ""));
  try {
    act(() =>
      root.render(
        <DndContext>
          <TaskCard
            task={task}
            tr={tr}
            ui={createContextModalUi(false)}
            syncBusy={false}
            selected={false}
            onSelectTask={open}
            onMoveTaskToStatus={move}
            onRestoreTask={vi.fn()}
          />
        </DndContext>,
      ),
    );
    const title = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) =>
      button.textContent?.includes("Review the detailed plan"),
    );
    expect(title).toBeDefined();
    expect(title?.closest("[data-task-id]")?.getAttribute("role")).toBeNull();
    act(() => title!.click());
    expect(open).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledWith(task);
    const handle = host.querySelector<HTMLButtonElement>(
      'button[aria-label="Move T1: Review the detailed plan"]',
    );
    expect(handle).not.toBeNull();
    // Read out in the page's language, not dnd-kit's English default.
    expect(handle?.getAttribute("aria-roledescription")).toBe("movable task");
    const action = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "Start",
    );
    act(() => action!.click());
    expect(move).toHaveBeenCalledWith(task, "active");
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});

it("uses the latest task revision even when all displayed fields are unchanged", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const open = vi.fn();
  const move = vi.fn();
  const restore = vi.fn();
  const tr = (_key: string, fallback: string) => fallback;
  const ui = createContextModalUi(false);
  const renderTask = (rev: number) => (
    <DndContext>
      <TaskCard
        task={{ id: "T1", title: "Plan", status: "planned", rev }}
        tr={tr}
        ui={ui}
        syncBusy={false}
        selected={false}
        onSelectTask={open}
        onMoveTaskToStatus={move}
        onRestoreTask={restore}
      />
    </DndContext>
  );
  try {
    act(() => root.render(renderTask(1)));
    act(() => root.render(renderTask(2)));
    const action = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "Start",
    );
    act(() => action!.click());
    expect(move).toHaveBeenCalledWith(expect.objectContaining({ rev: 2 }), "active");
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});

it("restores an archived task instead of moving it to planned", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const task = { id: "T1", title: "Archived work", status: "archived", rev: 3 };
  const move = vi.fn();
  const restore = vi.fn();
  try {
    act(() =>
      root.render(
        <DndContext>
          <TaskCard
            task={task}
            tr={(_key, fallback) => fallback}
            ui={createContextModalUi(false)}
            syncBusy={false}
            selected={false}
            onSelectTask={vi.fn()}
            onMoveTaskToStatus={move}
            onRestoreTask={restore}
          />
        </DndContext>,
      ),
    );
    const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (item) => item.textContent === "Restore",
    );
    expect(button).toBeDefined();
    act(() => button!.click());
    expect(restore).toHaveBeenCalledExactlyOnceWith(task);
    expect(move).not.toHaveBeenCalled();
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});
