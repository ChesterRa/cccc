// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { DragEndEvent } from "@dnd-kit/core";
import type { Task } from "../../types";
import type { TaskBoard } from "./coordination/TaskBoard";
import type { TaskEditorPanel } from "./coordination/TaskEditorPanel";

type BoardProps = ComponentProps<typeof TaskBoard>;
type EditorProps = ComponentProps<typeof TaskEditorPanel>;

const parent: Task = { id: "T001", title: "Release", status: "active" };
const first: Task = { id: "T002", title: "Draft notes", status: "planned", parent_id: "T001" };
const second: Task = { id: "T003", title: "Review copy", status: "planned", parent_id: "T001" };

const mocks = vi.hoisted(() => ({
  board: null as null | BoardProps,
  editor: null as null | EditorProps,
  boardTasks: [] as Task[],
  taskMap: new Map<string, Task>(),
  loadTaskDetail: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) => opts?.defaultValue || key,
  }),
}));

vi.mock("../../services/api", () => ({
  apiJson: vi.fn(),
  addCoordinationNote: vi.fn(),
  contextSync: vi.fn(),
  deleteCoordinationTask: vi.fn(),
  updateCoordinationBrief: vi.fn(),
  updateCoordinationTask: vi.fn(),
}));

vi.mock("../../features/contextModal/useTaskBoardData", () => ({
  useTaskBoardData: () => ({
    columns: Object.fromEntries(
      ["planned", "active", "done", "archived"].map((status) => [
        status,
        { items: [], totalCount: 0, hasMore: false, nextOffset: 0, loading: false },
      ]),
    ),
    board: Object.fromEntries(
      ["planned", "active", "done", "archived"].map((status) => [
        status,
        mocks.boardTasks.filter((task) => task.status === status),
      ]),
    ),
    taskIndex: [parent, first, second],
    taskMap: mocks.taskMap,
    tasksSummary: { total: 3, planned: 2, active: 1, done: 0, archived: 0 },
    facets: { assignees: [], unassigned: 0, blocked: 0, waitingUser: 0, pendingHandoffs: 0 },
    error: "",
    loading: false,
    deleteInfoById: {},
    refresh: vi.fn(async () => {}),
    loadMore: vi.fn(async () => {}),
    loadTaskDetail: mocks.loadTaskDetail,
    forgetTask: vi.fn(),
  }),
}));

vi.mock("./coordination/TaskBoard", () => ({
  TaskBoard: (props: BoardProps) => {
    mocks.board = props;
    return null;
  },
}));

vi.mock("./coordination/TaskEditorPanel", () => ({
  TaskEditorPanel: (props: EditorProps) => {
    mocks.editor = props;
    return null;
  },
}));

vi.mock("../modals/settings/CapabilitiesTab", () => ({ CapabilitiesTab: () => null }));

import { ContextModal } from "./index";
import { contextSync, updateCoordinationTask } from "../../services/api";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function deferred() {
  let resolve!: (task: Task | null) => void;
  const promise = new Promise<Task | null>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

const confirm = vi.fn(() => false);
const host = document.createElement("div");
let root: ReturnType<typeof createRoot>;

function editor(): EditorProps {
  if (!mocks.editor) throw new Error("task editor was not rendered");
  return mocks.editor;
}

async function openParent() {
  root = createRoot(host);
  await act(async () =>
    root.render(
      <ContextModal
        isOpen
        onClose={() => {}}
        groupId="g_subtasks"
        groupTitle="g_subtasks"
        context={null}
        onOpenContext={async () => {}}
        onSyncContext={async () => {}}
        isDark={false}
      />,
    ),
  );
  await act(async () => mocks.board?.onSelectTask(parent));
  expect(editor().selectedTask?.id).toBe(parent.id);
}

async function resolveDetail(pending: ReturnType<typeof deferred>, task: Task) {
  await act(async () => {
    mocks.taskMap.set(task.id, task);
    pending.resolve(task);
  });
}

beforeEach(() => {
  document.body.append(host);
  mocks.editor = null;
  mocks.boardTasks = [parent, first, second];
  mocks.taskMap = new Map([[parent.id, parent]]);
  mocks.loadTaskDetail.mockReset();
  vi.mocked(updateCoordinationTask).mockReset();
  vi.mocked(contextSync).mockReset();
  const rejected = { ok: false as const, error: { code: "fixture_stop", message: "Inspect only" } };
  vi.mocked(updateCoordinationTask).mockResolvedValue(rejected);
  vi.mocked(contextSync).mockResolvedValue(rejected);
  confirm.mockReset().mockReturnValue(false);
  Object.defineProperty(window, "confirm", { value: confirm, configurable: true });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("keeps parent edits made while an unloaded subtask is still loading", async () => {
  await openParent();
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);

  await act(async () => editor().onOpenTask(first.id));
  await act(async () =>
    editor().setTaskDraft((draft) => (draft ? { ...draft, title: "Unsaved edit" } : draft)),
  );
  await resolveDetail(pending, first);

  expect(editor().selectedTask?.id).toBe(parent.id);
  expect(editor().taskDraft?.title).toBe("Unsaved edit");
  expect(confirm).not.toHaveBeenCalled();
});

it("opens the most recently clicked subtask when an earlier load finishes first", async () => {
  await openParent();
  const loads = new Map([
    [first.id, deferred()],
    [second.id, deferred()],
  ]);
  mocks.loadTaskDetail.mockImplementation((taskId: string) => loads.get(taskId)?.promise);

  await act(async () => editor().onOpenTask(first.id));
  await act(async () => editor().onOpenTask(second.id));
  await resolveDetail(loads.get(first.id)!, first);
  await resolveDetail(loads.get(second.id)!, second);

  expect(editor().selectedTask?.id).toBe(second.id);
});

it("asks about unsaved edits when the subtask is clicked, not when it arrives", async () => {
  await openParent();
  await act(async () =>
    editor().setTaskDraft((draft) => (draft ? { ...draft, title: "Unsaved edit" } : draft)),
  );

  await act(async () => editor().onOpenTask(first.id));
  expect(confirm).toHaveBeenCalledTimes(1);
  expect(mocks.loadTaskDetail).not.toHaveBeenCalledWith(first.id);
  expect(editor().taskDraft?.title).toBe("Unsaved edit");

  confirm.mockReturnValue(true);
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);
  await act(async () => editor().onOpenTask(first.id));
  await resolveDetail(pending, first);

  expect(confirm).toHaveBeenCalledTimes(2);
  expect(editor().selectedTask?.id).toBe(first.id);
});

it("does not reopen the editor when a subtask finishes loading after it closed", async () => {
  await openParent();
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);

  await act(async () => editor().onOpenTask(first.id));
  await act(async () => editor().onClose());
  expect(mocks.board?.selectedTaskId).toBe("");
  await resolveDetail(pending, first);

  // The editor is only mounted while visible, so the always-rendered board shows the selection.
  expect(mocks.board?.selectedTaskId).toBe("");
});

it("refreshes a cached subtask before drafting and saving its current notes", async () => {
  await openParent();
  const cached: Task = { ...first, notes: "Old cached notes" };
  const current: Task = { ...first, notes: "New notes from another actor" };
  mocks.taskMap.set(first.id, cached);
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);

  await act(async () => editor().onOpenTask(first.id));
  expect(mocks.loadTaskDetail).toHaveBeenCalledWith(first.id);
  await resolveDetail(pending, current);

  expect(editor().selectedTask?.id).toBe(first.id);
  expect(editor().taskDraft?.notes).toBe(current.notes);
  vi.mocked(updateCoordinationTask).mockResolvedValue({
    ok: false,
    error: { code: "fixture_stop", message: "Only inspect the submitted task" },
  });
  await act(async () => editor().onSaveTask());
  expect(updateCoordinationTask).toHaveBeenCalledWith(
    "g_subtasks",
    expect.objectContaining({ id: first.id, notes: current.notes }),
  );
});

it("does not fall back to cached notes when the detail cannot be refreshed", async () => {
  await openParent();
  mocks.taskMap.set(first.id, { ...first, notes: "Stale notes" });
  mocks.loadTaskDetail.mockResolvedValue(null);

  await act(async () => editor().onOpenTask(first.id));

  expect(editor().selectedTask?.id).toBe(parent.id);
  expect(updateCoordinationTask).not.toHaveBeenCalled();
});

async function dragTo(taskId: string, status: string) {
  await act(async () =>
    mocks.board?.onDragEnd({
      active: { id: `task:${taskId}` },
      over: { id: `column:${status}` },
    } as DragEndEvent),
  );
}

it("changes only status when dragging a task with stale cached notes", async () => {
  await openParent();
  mocks.taskMap.set(first.id, { ...first, notes: "Old cached notes" });

  await dragTo(first.id, "active");

  expect(contextSync).toHaveBeenCalledWith("g_subtasks", [
    { op: "task.move", task_id: first.id, status: "active" },
  ]);
  expect(updateCoordinationTask).not.toHaveBeenCalled();
});

it("loads current notes before opening the editor for incomplete closeout", async () => {
  await openParent();
  const stale: Task = { ...first, task_type: "standard", notes: "Old cached notes" };
  const current: Task = { ...stale, notes: "New evidence from another actor" };
  mocks.taskMap.set(first.id, stale);
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);

  await dragTo(first.id, "done");
  expect(editor().selectedTask?.id).toBe(parent.id);
  await resolveDetail(pending, current);

  expect(editor().selectedTask?.id).toBe(first.id);
  expect(editor().taskDraft?.notes).toBe(current.notes);
  expect(editor().taskDraft?.status).toBe("done");
  expect(contextSync).not.toHaveBeenCalled();
  expect(updateCoordinationTask).not.toHaveBeenCalled();
});

it("does not bypass closeout requirements introduced after the task was cached", async () => {
  await openParent();
  mocks.taskMap.set(first.id, { ...first, task_type: "free" });
  const current: Task = { ...first, task_type: "standard", notes: "Needs closeout" };
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);

  await dragTo(first.id, "done");
  await resolveDetail(pending, current);

  expect(editor().selectedTask?.id).toBe(first.id);
  expect(editor().taskDraft?.taskType).toBe("standard");
  expect(editor().taskDraft?.notes).toBe(current.notes);
  expect(contextSync).not.toHaveBeenCalled();
  expect(updateCoordinationTask).not.toHaveBeenCalled();
});

it("marks a task done when the latest details already have complete closeout", async () => {
  await openParent();
  mocks.taskMap.set(first.id, { ...first, task_type: "standard" });
  const current: Task = {
    ...first,
    task_type: "standard",
    outcome: "Verified",
    notes: "Closeout Verdict:\nDONE\n\nVerification Summary:\nFocused checks passed",
  };
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);

  await dragTo(first.id, "done");
  await resolveDetail(pending, current);

  expect(contextSync).toHaveBeenCalledWith("g_subtasks", [
    { op: "task.move", task_id: first.id, status: "done" },
  ]);
  expect(updateCoordinationTask).not.toHaveBeenCalled();
  expect(editor().selectedTask?.id).toBe(parent.id);
});

it("keeps edits made while a completion check is loading", async () => {
  await openParent();
  mocks.taskMap.set(first.id, { ...first, task_type: "standard" });
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);

  await dragTo(first.id, "done");
  await act(async () =>
    editor().setTaskDraft((draft) => (draft ? { ...draft, title: "Keep my edit" } : draft)),
  );
  await resolveDetail(pending, { ...first, task_type: "standard", notes: "Fresh details" });

  expect(editor().selectedTask?.id).toBe(parent.id);
  expect(editor().taskDraft?.title).toBe("Keep my edit");
  expect(confirm).not.toHaveBeenCalled();
  expect(contextSync).not.toHaveBeenCalled();
});

it("keeps unsaved changes in the same task for an explicit editor save", async () => {
  await openParent();
  await act(async () =>
    editor().setTaskDraft((draft) => (draft ? { ...draft, notes: "Unsaved evidence" } : draft)),
  );

  await act(async () => mocks.board?.onMoveTaskToStatus(parent, "done"));

  expect(editor().selectedTask?.id).toBe(parent.id);
  expect(editor().taskDraft?.notes).toBe("Unsaved evidence");
  expect(editor().taskDraft?.status).toBe("done");
  expect(confirm).not.toHaveBeenCalled();
  expect(contextSync).not.toHaveBeenCalled();
  expect(updateCoordinationTask).not.toHaveBeenCalled();
});

it("uses the visible card status when a cached detail still has the target status", async () => {
  mocks.boardTasks = [parent, { ...first, status: "active" }, second];
  await openParent();
  mocks.taskMap.set(first.id, first);

  await dragTo(first.id, "planned");

  expect(contextSync).toHaveBeenCalledWith("g_subtasks", [
    { op: "task.move", task_id: first.id, status: "planned" },
  ]);
});

it("does not cancel pending navigation when a different card only changes status", async () => {
  await openParent();
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);
  await act(async () => editor().onOpenTask(first.id));
  mocks.taskMap.set(second.id, second);

  await dragTo(second.id, "active");
  await resolveDetail(pending, first);

  expect(editor().selectedTask?.id).toBe(first.id);
});

it("allows a valid done transition without disturbing edits to another task", async () => {
  await openParent();
  const pending = deferred();
  mocks.loadTaskDetail.mockReturnValue(pending.promise);
  mocks.taskMap.set(first.id, first);
  await dragTo(first.id, "done");
  await act(async () =>
    editor().setTaskDraft((draft) => (draft ? { ...draft, title: "Keep this edit" } : draft)),
  );
  await resolveDetail(pending, { ...first, task_type: "free" });

  expect(contextSync).toHaveBeenCalledWith("g_subtasks", [
    { op: "task.move", task_id: first.id, status: "done" },
  ]);
  expect(editor().selectedTask?.id).toBe(parent.id);
  expect(editor().taskDraft?.title).toBe("Keep this edit");
  expect(confirm).not.toHaveBeenCalled();
});
