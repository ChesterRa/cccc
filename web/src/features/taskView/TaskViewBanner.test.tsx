// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { TaskViewBanner } from "./TaskViewBanner";
import { getTaskView, useEndTaskViewOnGroupChange, useTaskViewStore } from "./taskViewStore";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}:${JSON.stringify(options)}` : key,
  }),
}));

let host: HTMLDivElement;
let root: Root;
const handlers = { onDetails: vi.fn(), onRetry: vi.fn(), onExit: vi.fn() };

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.clearAllMocks();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  useTaskViewStore.getState().close();
});

function render(extra: Partial<Parameters<typeof TaskViewBanner>[0]> = {}) {
  act(() =>
    root.render(
      <TaskViewBanner
        taskId="T98"
        title="Ship it"
        count={3}
        loading={false}
        error={false}
        truncated={false}
        contextIncomplete={false}
        {...handlers}
        {...extra}
      />,
    ),
  );
}
const button = (text: string) =>
  [...host.querySelectorAll("button")].find((item) => item.textContent === text)!;
const escape = (target: Element | Document = document) =>
  act(() => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });

it("names the task, counts its messages and leads to its details and out", () => {
  render();
  expect(host.textContent).toContain("T98");
  expect(host.textContent).toContain("Ship it");
  expect(host.querySelector("[data-task-view-count]")?.textContent).toBe(
    'taskView.count:{"count":3}',
  );
  expect(host.querySelector("[data-task-view-note]")).toBeNull();
  act(() => button("taskView.details").click());
  expect(handlers.onDetails).toHaveBeenCalledTimes(1);
  act(() => button("taskView.exit").click());
  expect(handlers.onExit).toHaveBeenCalledTimes(1);
});

it("leaves on Escape, except while a field has it", () => {
  render();
  const field = document.createElement("textarea");
  document.body.append(field);
  escape(field);
  expect(handlers.onExit).not.toHaveBeenCalled();
  field.remove();
  escape();
  expect(handlers.onExit).toHaveBeenCalledTimes(1);
});

it("says when the view is partial, and offers a retry only after a failed read", () => {
  render({ truncated: true });
  expect(host.querySelector("[data-task-view-note]")?.getAttribute("data-task-view-note")).toBe(
    "truncated",
  );
  expect(button("taskView.retry")).toBeUndefined();
  render({ contextIncomplete: true });
  expect(host.querySelector("[data-task-view-note]")?.textContent).toBe(
    "taskView.contextIncomplete",
  );
  render({ error: true });
  act(() => button("taskView.retry").click());
  expect(handlers.onRetry).toHaveBeenCalledTimes(1);
});

it("shows loading in place of the count", () => {
  render({ loading: true });
  expect(host.querySelector("[data-task-view-count]")?.textContent).toBe("taskView.loading");
});

it("keeps a task view to the Group it was opened in", () => {
  useTaskViewStore.getState().open("g1", " T98 ", " Ship it ");
  expect(getTaskView("g1")).toEqual({ groupId: "g1", taskId: "T98", title: "Ship it" });
  expect(getTaskView("g2")).toBeNull();
  useTaskViewStore.getState().open("g1", "", "nothing");
  expect(getTaskView("g1")?.taskId).toBe("T98");
  useTaskViewStore.getState().close();
  expect(getTaskView("g1")).toBeNull();
});

it("ends the view when the chat turns to another Group, and not on the way back", () => {
  function Chat({ groupId }: { groupId: string }) {
    useEndTaskViewOnGroupChange(groupId);
    return null;
  }
  act(() => root.render(<Chat groupId="g1" />));
  act(() => useTaskViewStore.getState().open("g1", "T98"));
  expect(getTaskView("g1")?.taskId).toBe("T98");
  act(() => root.render(<Chat groupId="g2" />));
  act(() => root.render(<Chat groupId="g1" />));
  expect(getTaskView("g1")).toBeNull();
});
