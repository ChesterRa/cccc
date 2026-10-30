// @vitest-environment happy-dom
import { DndContext } from "@dnd-kit/core";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { Task } from "../../../types";
import { createContextModalUi } from "../ui";
import { TaskCard } from "./TaskCard";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ i18n: { language: "en" } }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const tr = (_key: string, fallback: string, values?: Record<string, unknown>) =>
  fallback.replace(/\{\{(\w+)\}\}/g, (_match, key: string) => String(values?.[key] ?? ""));

function cardText(task: Task): string {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
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
            onSelectTask={vi.fn()}
            onMoveTaskToStatus={vi.fn()}
            onRestoreTask={vi.fn()}
          />
        </DndContext>,
      ),
    );
    return host.querySelector("[data-task-id]")?.textContent || "";
  } finally {
    act(() => root.unmount());
    host.remove();
  }
}

it("uses separate operable controls for opening and moving a task", () => {
  const host = document.createElement("div");
  const globalStyle = document.createElement("style");
  // Mirror the app's unlayered rule: the drag handle must override it, while
  // the ordinary title button keeps the browser's normal touch gestures.
  globalStyle.textContent = "button { touch-action: manipulation; }";
  document.head.append(globalStyle);
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
    expect(getComputedStyle(handle!).touchAction).toBe("none");
    expect(getComputedStyle(title!).touchAction).toBe("manipulation");
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
    globalStyle.remove();
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

it("shows attention only while a task is live, preserving it for a later reopen", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const restore = vi.fn();
  const tr = (_key: string, fallback: string) => fallback;
  const ui = createContextModalUi(false);
  const task: Task = {
    id: "T1",
    title: "Finish the review",
    status: "active",
    blocked_by: ["T2"],
    waiting_on: "user",
    handoff_to: "agent-1",
  };
  const renderStatus = (status: string) => (
    <DndContext>
      <TaskCard
        task={{ ...task, status }}
        tr={tr}
        ui={ui}
        syncBusy={false}
        selected={false}
        onSelectTask={vi.fn()}
        onMoveTaskToStatus={vi.fn()}
        onRestoreTask={restore}
      />
    </DndContext>
  );
  const expectAttention = (visible: boolean) => {
    const card = host.querySelector<HTMLElement>('[data-task-id="T1"]')!;
    expect(card.className.includes("border-rose-500/30")).toBe(visible);
    expect(card.textContent?.includes("Blocked: T2")).toBe(visible);
    expect(card.textContent?.includes("Waiting on user")).toBe(visible);
    expect(card.textContent?.includes("Handoff → agent-1")).toBe(visible);
  };
  try {
    act(() => root.render(renderStatus("active")));
    expectAttention(true);
    act(() => root.render(renderStatus("done")));
    expectAttention(false);
    act(() => root.render(renderStatus("archived")));
    expectAttention(false);
    const button = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (item) => item.textContent === "Restore",
    );
    act(() => button!.click());
    expect(restore).toHaveBeenCalledWith({ ...task, status: "archived" });
    act(() => root.render(renderStatus("active")));
    expectAttention(true);
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});

it("shows the first open checklist item of an unfinished task that has no summary", () => {
  const checklist = [
    { id: "1", text: "Write the parser", status: "done" },
    { id: "2", text: "Wait for the vendor", status: "blocked" },
    { id: "3", text: "  ", status: "pending" },
    { id: "4", text: "Review the parser" },
    { id: "5", text: "Ship it", status: "pending" },
  ];
  expect(cardText({ id: "T1", title: "Parser", status: "active", checklist })).toContain(
    "To do: Review the parser",
  );
  // A summary keeps its place, and a finished task has nothing left to do.
  const summarized = { id: "T1", title: "Parser", status: "active", checklist, outcome: "Merged" };
  expect(cardText(summarized)).not.toContain("To do:");
  expect(cardText({ id: "T1", title: "Parser", status: "done", checklist })).not.toContain(
    "To do:",
  );
});

it("tells how long ago a task changed, and nothing for a time it cannot place", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-29T06:00:00Z"));
  try {
    const at = (updated_at: string) => cardText({ id: "T1", title: "Parser", updated_at });
    expect(at("2026-10-29T04:00:00Z")).toContain("T1 · Updated 2 hours ago");
    expect(at("2026-10-29T05:59:30Z")).toContain("T1 · Updated just now");
    // Ahead of this device's clock, or unreadable: no claim at all.
    expect(at("2026-10-29T06:05:00Z")).not.toContain("Updated");
    expect(at("yesterday")).not.toContain("Updated");
  } finally {
    vi.useRealTimers();
  }
});

it("quotes a search match the card does not otherwise show", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (task: Task, searchQuery: string) =>
    act(() =>
      root.render(
        <DndContext>
          <TaskCard
            task={task}
            tr={tr}
            ui={createContextModalUi(false)}
            syncBusy={false}
            searchQuery={searchQuery}
            selected={false}
            onSelectTask={vi.fn()}
            onMoveTaskToStatus={vi.fn()}
            onRestoreTask={vi.fn()}
          />
        </DndContext>,
      ),
    );
  const quote = () => host.querySelector("mark")?.parentElement?.textContent ?? null;
  const notes = "Root cause: the deploy script skips the cache.";
  const live: Task = { id: "T4", title: "Fix deploys", status: "active", notes };
  try {
    render(live, "");
    expect(host.querySelector("mark")).toBeNull();
    render(live, "CACHE");
    expect(host.querySelector("mark")?.textContent).toBe("cache");
    expect(quote()).toBe("Notes: …script skips the cache.");

    // A match in the title, or in a handoff the live card already shows, needs no quote.
    render(live, "deploys");
    expect(host.querySelector("mark")).toBeNull();
    render({ ...live, handoff_to: "reviewer" }, "review");
    expect(host.querySelector("mark")).toBeNull();
    // A Done card hides its handoff, so there the quote says where the match is.
    render({ ...live, status: "done", handoff_to: "reviewer" }, "review");
    expect(quote()).toBe("Handoff → reviewer");
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});
