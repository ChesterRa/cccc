// @vitest-environment happy-dom
import { act, useState, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Task } from "../../../src/types";
import { evaluateTaskWorkflow, getTaskTypeDefinition } from "../../../src/utils/taskWorkflow";
import { taskToDraft, type TaskDraft } from "../../../src/components/ContextModal/model";
import { createContextModalUi } from "../../../src/components/ContextModal/ui";
import { TaskEditorPanel } from "../../../src/components/ContextModal/coordination/TaskEditorPanel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const tr = (_key: string, fallback: string, options?: Record<string, unknown>) =>
  fallback.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => String(options?.[name] ?? ""));

const task: Task = { id: "T7", title: "Ship notifications", status: "active", rev: 4 };

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

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

function Editor(props: Props) {
  const [draft, setDraft] = useState<TaskDraft | null>(() => taskToDraft(task));
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
      assigneeChoices={[]}
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

function render(props: Props) {
  act(() => root.render(<Editor {...props} />));
}

function button(label: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find((item) => item.textContent?.trim() === label);
}

function sourceRow(): HTMLElement | null {
  return host.querySelector("[data-task-backlog-source]");
}

it("links a task to the entry it came from and offers to update or put it back", () => {
  const onShowSource = vi.fn();
  const onUpdateSource = vi.fn();
  const onPutBack = vi.fn();
  render({ backlogLink: { sourceTitle: "Sync speed", onShowSource, onUpdateSource, onPutBack } });

  expect(sourceRow()?.textContent).toContain("From the backlog:");
  act(() => button("Sync speed")?.click());
  act(() => button("Update the entry…")?.click());
  act(() => button("Put back in the backlog")?.click());
  expect(onShowSource).toHaveBeenCalledTimes(1);
  expect(onUpdateSource).toHaveBeenCalledTimes(1);
  expect(onPutBack).toHaveBeenCalledTimes(1);
});

it("says when the entry is not in the current file, and waits while it is read", () => {
  render({ backlogLink: { sourceTitle: null } });
  expect(sourceRow()?.textContent).toBe(
    "This task came from a backlog entry that is not in the current backlog file.",
  );

  render({ backlogLink: { sourceTitle: "" } });
  expect(sourceRow()?.textContent).toBe("From the backlog:…");
});

it("offers only putting back for a task from no entry, and nothing in a new task", () => {
  render({ backlogLink: { onPutBack: vi.fn() } });
  expect(sourceRow()).toBeNull();
  expect(button("Put back in the backlog")).toBeDefined();

  render({
    taskEditorMode: "create",
    backlogLink: { sourceTitle: "Sync speed", onPutBack: vi.fn() },
  });
  expect(sourceRow()).toBeNull();
  expect(button("Put back in the backlog")).toBeUndefined();
});

it("asks for unsaved changes to be settled before going to the backlog", () => {
  render({
    hasTaskUnsaved: true,
    backlogLink: { sourceTitle: "Sync speed", onUpdateSource: vi.fn(), onPutBack: vi.fn() },
  });
  for (const label of ["Update the entry…", "Put back in the backlog"]) {
    expect(button(label)?.disabled).toBe(true);
    expect(button(label)?.title).toBe("Save or discard your changes first.");
  }
});
