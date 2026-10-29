// @vitest-environment happy-dom
import { act, useState, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Task } from "../../../types";
import { evaluateTaskWorkflow, getTaskTypeDefinition } from "../../../utils/taskWorkflow";
import { taskToDraft, type TaskDraft } from "../model";
import { createContextModalUi } from "../ui";
import { TaskEditorPanel } from "./TaskEditorPanel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const tr = (_key: string, fallback: string, options?: Record<string, unknown>) =>
  fallback.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => String(options?.[name] ?? ""));

const task: Task = {
  id: "T7",
  title: "Ship notifications",
  status: "active",
  rev: 4,
  checklist: [
    { id: "item-1", text: "Write the sender", status: "done" },
    { id: "item-3", text: "Test on iPhone", status: "in_progress" },
  ],
};

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let current: TaskDraft | null;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

type Props = Partial<ComponentProps<typeof TaskEditorPanel>>;

function Editor({ initial = task, ...props }: Props & { initial?: Task }) {
  const [draft, setDraft] = useState<TaskDraft | null>(() => taskToDraft(initial));
  current = draft;
  return (
    <TaskEditorPanel
      tr={tr}
      ui={createContextModalUi(false)}
      taskEditorMode="edit"
      taskDraft={draft}
      hasTaskUnsaved={false}
      syncBusy={false}
      error=""
      taskGone={false}
      taskStale=""
      selectedTask={task}
      subtasks={[]}
      assigneeChoices={["claude-1", "codex-1"]}
      parentChoices={[]}
      selectedTaskDeleteInfo={{ allowed: false, total: 1, reason: "self_history" }}
      selectedTaskDeleteHint=""
      taskWorkflowCoverage={evaluateTaskWorkflow({ status: draft?.status })}
      taskTypeId="standard"
      selectedTaskType={getTaskTypeDefinition("standard")}
      setTaskDraft={setDraft}
      onTaskTypeChange={vi.fn()}
      onResetTask={vi.fn()}
      onClose={vi.fn()}
      onDeleteSelectedTask={vi.fn()}
      onSaveTask={vi.fn()}
      onOpenTask={vi.fn()}
      onAddSubtask={vi.fn()}
      {...props}
    />
  );
}

function render(props: Props = {}, initial?: Task) {
  act(() => root.render(<Editor {...props} initial={initial} />));
}

function button(label: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find(
    (item) => item.textContent?.trim() === label || item.getAttribute("aria-label") === label,
  );
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
}

function itemInput(n: number): HTMLTextAreaElement {
  // A text area, so a long item wraps instead of running off the edge.
  const input = host.querySelector<HTMLTextAreaElement>(
    `textarea[aria-label="Checklist item ${n}"]`,
  );
  if (!input) throw new Error(`Missing checklist item ${n}`);
  return input;
}

function type(input: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype =
    input instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
  act(() => {
    setter?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

it("lets long text grow with the editor rather than be dragged over", () => {
  render();
  const fields = [...host.querySelectorAll("label")]
    .filter((label) =>
      ["Outcome", "Notes"].includes(label.querySelector("span")?.textContent || ""),
    )
    .map((label) => label.querySelector("textarea"));
  // Inline, so the stylesheet order of resize classes cannot bring the handle back.
  for (const field of [...fields, itemInput(1)]) {
    expect(field?.style.resize).toBe("none");
    expect(field?.style.overflowY).toBe("hidden");
  }
  expect(fields).toHaveLength(2);
});

it("lets huge text scroll inside its box so later controls stay within reach", () => {
  render();
  const notes = [...host.querySelectorAll("label")]
    .find((label) => label.querySelector("span")?.textContent === "Notes")
    ?.querySelector("textarea");
  if (!notes) throw new Error("Missing notes");
  let contentHeight = 25_000;
  Object.defineProperty(notes, "scrollHeight", { configurable: true, get: () => contentHeight });
  type(notes, "x".repeat(500_000));
  expect(notes.style.height).toBe("10000px");
  expect(notes.style.overflowY).toBe("auto");

  // Back under the limit, it grows with the editor again.
  contentHeight = 320;
  type(notes, "Short again");
  expect(notes.style.height).toBe("320px");
  expect(notes.style.overflowY).toBe("hidden");
});

it("edits the checklist as items that keep their ids", () => {
  render();
  const boxes = host.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
  expect(boxes[0].checked).toBe(true);
  // In progress shows as neither done nor not done.
  expect(boxes[1].indeterminate).toBe(true);
  expect(host.textContent).toContain("1 of 2 done");

  act(() => button("Add item").click());
  // A new item takes an id no other item has, and has the focus.
  expect(current?.checklist.map((item) => item.id)).toEqual(["item-1", "item-3", "item-4"]);
  expect(document.activeElement).toBe(itemInput(3));
  type(itemInput(3), "Test on Android");

  act(() => boxes[1].click());
  act(() => button("Remove item 1").click());
  expect(current?.checklist).toEqual([
    { id: "item-3", text: "Test on iPhone", status: "done" },
    { id: "item-4", text: "Test on Android", status: "pending" },
  ]);
});

it("keeps a status it does not show until the item is ticked", () => {
  render(
    {},
    { ...task, checklist: [{ id: "item-1", text: "Waiting on legal", status: "blocked" }] },
  );
  const box = host.querySelector<HTMLInputElement>('input[type="checkbox"]');
  expect(box?.checked).toBe(false);
  // Shown as written beside the item, and read out with its checkbox.
  const shown = document.getElementById(box?.getAttribute("aria-describedby") || "");
  expect(shown?.textContent).toBe("blocked");
  expect(shown?.closest("li")).toBe(box?.closest("li"));
  type(itemInput(1), "Waiting on legal review");
  expect(current?.checklist[0].status).toBe("blocked");
  act(() => box?.click());
  expect(current?.checklist[0].status).toBe("done");
  expect(host.textContent).not.toContain("blocked");
});

it("tells a field's own text from no value, whatever the text", async () => {
  render({}, { ...task, priority: "__none__", assignee: "none" });
  const trigger = (label: string) =>
    host.querySelector<HTMLButtonElement>(`button[role="combobox"][aria-label="${label}"]`)!;
  const option = (text: string) =>
    [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(
      (item) => item.textContent === text,
    )!;
  // A list hands focus back to its field on a task after it has closed; until
  // then that focus would close a list opened in between, which no person is
  // fast enough to do.
  const choose = async (field: string, text: string) => {
    await act(async () => trigger(field).click());
    await act(async () => option(text).click());
    await act(() => new Promise((settle) => setTimeout(settle, 0)));
  };
  expect(trigger("Priority").textContent).toBe("__none__");
  expect(trigger("Assignee").textContent).toBe("none");

  // Choosing the value itself keeps it; choosing none clears it.
  await choose("Priority", "__none__");
  expect(current?.priority).toBe("__none__");
  await choose("Priority", "No priority");
  expect(current?.priority).toBe("");
  await choose("Assignee", "Unassigned");
  expect(current?.assignee).toBe("");
});

it("adds an item on Enter, but not on the Enter that ends an IME composition", () => {
  render();
  const input = itemInput(2);
  act(() => {
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, isComposing: true }),
    );
  });
  expect(current?.checklist).toHaveLength(2);
  act(() => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  expect(current?.checklist).toHaveLength(3);
  expect(document.activeElement).toBe(itemInput(3));
});

it("saves only an edit, and never a task deleted elsewhere", () => {
  render();
  expect(button("Save task").disabled).toBe(true);
  render({ hasTaskUnsaved: true });
  expect(button("Save task").disabled).toBe(false);
  expect(button("Discard changes")).toBeDefined();

  render({ hasTaskUnsaved: true, taskGone: true });
  expect(button("Save task").disabled).toBe(true);
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("no longer exists");
});

it("offers Delete only when it can be used, and says why not with the details", () => {
  const hint = "Only tasks that never left Planned can be deleted. Move it to Archived instead.";
  render({ selectedTaskDeleteHint: hint });
  expect([...host.querySelectorAll("button")].some((item) => item.textContent === "Delete")).toBe(
    false,
  );
  // The reason sits with the task's details, not in the footer kept on screen.
  const reason = [...host.querySelectorAll("p")].find((item) => item.textContent === hint);
  expect(reason?.closest(".overflow-y-auto")).not.toBeNull();

  const onDeleteSelectedTask = vi.fn();
  render({ selectedTaskDeleteInfo: { allowed: true, total: 1, reason: "" }, onDeleteSelectedTask });
  act(() => button("Delete").click());
  expect(onDeleteSelectedTask).toHaveBeenCalledOnce();
});

it("shows errors in the drawer and offers the latest version of a stale task", () => {
  const onResetTask = vi.fn();
  render({ error: "Task title is required.", taskStale: "changed", onResetTask });
  const alerts = [...host.querySelectorAll('[role="alert"]')].map((item) => item.textContent);
  expect(alerts[0]).toBe("Task title is required.");
  expect(alerts[1]).toContain("changed since you opened it");
  act(() => button("Reload latest version").click());
  expect(onResetTask).toHaveBeenCalledOnce();
});

it("says a save went through when it could not be read back, and saves again only after a reload", () => {
  render({ taskStale: "unconfirmed", hasTaskUnsaved: true });
  const alert = host.querySelector('[role="alert"]')?.textContent || "";
  expect(alert).toContain("Saved, but");
  expect(alert).not.toContain("changed since");
  expect(button("Save task").disabled).toBe(true);
  expect(button("Reload latest version").disabled).toBe(false);
});

it("holds still while saving", () => {
  render({ syncBusy: true, hasTaskUnsaved: true });
  expect(host.querySelector<HTMLInputElement>("input:not([type])")?.readOnly).toBe(true);
  expect(itemInput(1).readOnly).toBe(true);
  expect(button("Add item").disabled).toBe(true);
  expect(button("Saving…").disabled).toBe(true);
});

it("suggests a result for a finished task without requiring one", () => {
  render({ hasTaskUnsaved: true }, { ...task, status: "done" });
  expect(host.textContent).toContain("Add a short result");
  expect(button("Save task").disabled).toBe(false);
  const outcome = host.querySelector("textarea");
  if (!outcome) throw new Error("Missing outcome");
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  act(() => {
    setter?.call(outcome, "Shipped to all three platforms");
    outcome.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(current?.outcome).toBe("Shipped to all three platforms");
  expect(host.textContent).not.toContain("Add a short result");
});
