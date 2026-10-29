// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { DragEndEvent, DragStartEvent } from "@dnd-kit/core";
import type { Task } from "../../types";
import type { TaskDraft } from "./model";
import type { TaskBoard } from "./coordination/TaskBoard";
import type { TaskEditorPanel } from "./coordination/TaskEditorPanel";

type BoardProps = ComponentProps<typeof TaskBoard>;
type EditorProps = ComponentProps<typeof TaskEditorPanel>;

const parent: Task = { id: "T001", title: "Release", status: "active", rev: 1 };
const first: Task = {
  id: "T002",
  title: "Draft notes",
  status: "planned",
  parent_id: "T001",
  rev: 1,
};
const second: Task = {
  id: "T003",
  title: "Review copy",
  status: "planned",
  parent_id: "T001",
  rev: 1,
};

const mocks = vi.hoisted(() => ({
  board: null as null | BoardProps,
  editor: null as null | EditorProps,
  boardTasks: [] as Task[],
  taskMap: new Map<string, Task>(),
  deleteInfoById: {} as Record<string, unknown>,
  loadTaskDetail: vi.fn(),
  refresh: vi.fn(async () => {}),
  syncContext: vi.fn(async () => {}),
  close: vi.fn(),
  // As the board does: a forgotten task leaves it.
  forgetTask: vi.fn((taskId: string) => {
    mocks.taskMap.delete(taskId);
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string } & Record<string, unknown>) =>
      (opts?.defaultValue || key).replace(/\{\{(\w+)\}\}/g, (_match, name: string) =>
        String(opts?.[name] ?? ""),
      ),
  }),
}));

vi.mock("../../services/api", async () => ({
  isTaskRevision: (
    await vi.importActual<typeof import("../../services/api/base")>("../../services/api/base")
  ).isTaskRevision,
  apiJson: vi.fn(),
  addCoordinationNote: vi.fn(),
  contextSync: vi.fn(),
  deleteCoordinationTaskConfirmed: vi.fn(),
  moveCoordinationTask: vi.fn(),
  restoreCoordinationTask: vi.fn(),
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
    deleteInfoById: mocks.deleteInfoById,
    refresh: mocks.refresh,
    loadMore: vi.fn(async () => {}),
    loadTaskDetail: mocks.loadTaskDetail,
    forgetTask: mocks.forgetTask,
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
import {
  finishWorkspaceNavigation,
  requestWorkspaceNavigation,
  useWorkspaceNavigation,
} from "../../stores/workspaceNavigation";
import {
  contextSync,
  deleteCoordinationTaskConfirmed,
  moveCoordinationTask,
  restoreCoordinationTask,
  updateCoordinationTask,
} from "../../services/api";

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

// A new element each time: React skips rendering an element it already has.
const modal = () => (
  <ContextModal
    isOpen
    onClose={mocks.close}
    groupId="g_subtasks"
    groupTitle="g_subtasks"
    context={null}
    onOpenContext={async () => {}}
    onSyncContext={() => mocks.syncContext()}
    isDark={false}
  />
);

async function openParent() {
  root = createRoot(host);
  await act(async () => root.render(modal()));
  await act(async () => mocks.board?.onSelectTask(parent));
  expect(editor().selectedTask?.id).toBe(parent.id);
}

/** Render again after the board data changed, as a refresh would. */
async function rerender() {
  await act(async () => root.render(modal()));
}

async function edit(patch: Partial<TaskDraft>) {
  await act(async () => editor().setTaskDraft((draft) => (draft ? { ...draft, ...patch } : draft)));
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
  mocks.deleteInfoById = {};
  mocks.loadTaskDetail.mockReset();
  mocks.refresh.mockClear();
  mocks.syncContext.mockReset().mockResolvedValue(undefined);
  mocks.close.mockReset();
  vi.mocked(updateCoordinationTask).mockReset();
  vi.mocked(contextSync).mockReset();
  vi.mocked(moveCoordinationTask).mockReset();
  vi.mocked(restoreCoordinationTask).mockReset();
  vi.mocked(deleteCoordinationTaskConfirmed).mockReset();
  const rejected = { ok: false as const, error: { code: "fixture_stop", message: "Inspect only" } };
  vi.mocked(updateCoordinationTask).mockResolvedValue(rejected);
  vi.mocked(contextSync).mockResolvedValue(rejected);
  vi.mocked(moveCoordinationTask).mockResolvedValue(rejected);
  vi.mocked(restoreCoordinationTask).mockResolvedValue(rejected);
  vi.mocked(deleteCoordinationTaskConfirmed).mockResolvedValue(rejected);
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
  await edit({ title: "Renamed" });
  await act(async () => editor().onSaveTask());
  // Only the edit goes out; the notes it did not touch are not rewritten.
  expect(updateCoordinationTask).toHaveBeenCalledWith(
    "g_subtasks",
    first.id,
    { title: "Renamed" },
    current.rev,
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

it("moves a card fenced on the revision the board showed", async () => {
  await openParent();
  mocks.taskMap.set(first.id, { ...first, notes: "Old cached notes" });

  await dragTo(first.id, "active");

  expect(moveCoordinationTask).toHaveBeenCalledExactlyOnceWith(
    "g_subtasks",
    first.id,
    "active",
    first.rev,
  );
  expect(contextSync).not.toHaveBeenCalled();
  expect(updateCoordinationTask).not.toHaveBeenCalled();
});

it("moves a standard task to done without asking for closeout notes", async () => {
  mocks.boardTasks = [parent, { ...first, task_type: "standard" }, second];
  await openParent();

  await dragTo(first.id, "done");

  expect(moveCoordinationTask).toHaveBeenCalledExactlyOnceWith(
    "g_subtasks",
    first.id,
    "done",
    first.rev,
  );
  expect(mocks.loadTaskDetail).not.toHaveBeenCalledWith(first.id);
  expect(editor().selectedTask?.id).toBe(parent.id);
});

const alerts = () => [...host.querySelectorAll('[role="alert"]')].map((alert) => alert.textContent);

it("reads the board again when a card changed before it was moved", async () => {
  await openParent();
  vi.mocked(moveCoordinationTask).mockResolvedValue({
    ok: false,
    error: { code: "task_conflict", message: "conflict" },
  });
  mocks.refresh.mockClear();

  await dragTo(first.id, "active");

  expect(mocks.refresh).toHaveBeenCalled();
  // On the board: the open editor is about another task.
  expect(alerts()).toEqual([expect.stringContaining("T002 has changed since the board showed it")]);
  expect(editor().error).toBe("");
});

it("uses the visible card status when a cached detail still has the target status", async () => {
  mocks.boardTasks = [parent, { ...first, status: "active" }, second];
  await openParent();
  mocks.taskMap.set(first.id, first);

  await dragTo(first.id, "planned");

  expect(moveCoordinationTask).toHaveBeenCalledWith("g_subtasks", first.id, "planned", first.rev);
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

it("moves another card without disturbing the edits in the open task", async () => {
  await openParent();
  await edit({ title: "Keep this edit" });

  await dragTo(first.id, "done");

  expect(moveCoordinationTask).toHaveBeenCalledWith("g_subtasks", first.id, "done", first.rev);
  expect(editor().selectedTask?.id).toBe(parent.id);
  expect(editor().taskDraft?.title).toBe("Keep this edit");
  expect(confirm).not.toHaveBeenCalled();
});

it("saves fenced on the revision the draft was made from", async () => {
  await openParent();
  await edit({ title: "Renamed" });

  await act(async () => editor().onSaveTask());

  expect(updateCoordinationTask).toHaveBeenCalledExactlyOnceWith(
    "g_subtasks",
    parent.id,
    { title: "Renamed" },
    parent.rev,
  );
});

it("follows changes made elsewhere while the draft has no edits", async () => {
  await openParent();

  mocks.taskMap.set(parent.id, { ...parent, rev: 2, notes: "From an agent" });
  await rerender();

  expect(editor().taskDraft?.notes).toBe("From an agent");
  expect(editor().hasTaskUnsaved).toBe(false);
  expect(editor().taskStale).toBe("");
});

it("keeps edits over a task changed elsewhere and does not overwrite it", async () => {
  await openParent();
  await edit({ title: "Mine" });

  mocks.taskMap.set(parent.id, { ...parent, rev: 2, notes: "From an agent" });
  await rerender();

  expect(editor().taskDraft?.title).toBe("Mine");
  expect(editor().taskDraft?.notes).toBe("");
  expect(editor().taskStale).toBe("changed");

  vi.mocked(updateCoordinationTask).mockResolvedValue({
    ok: false,
    error: { code: "task_conflict", message: "conflict" },
  });
  await act(async () => editor().onSaveTask());
  // Fenced on what the draft was made from, so the daemon refuses it.
  expect(updateCoordinationTask).toHaveBeenCalledWith(
    "g_subtasks",
    parent.id,
    { title: "Mine" },
    parent.rev,
  );
  expect(editor().taskDraft?.title).toBe("Mine");
  expect(editor().taskStale).toBe("changed");

  // Reloading drops the edits for the latest version.
  mocks.loadTaskDetail.mockResolvedValue(mocks.taskMap.get(parent.id));
  await act(async () => editor().onResetTask());
  expect(editor().taskDraft?.title).toBe(parent.title);
  expect(editor().taskDraft?.notes).toBe("From an agent");
  expect(editor().taskStale).toBe("");
});

it("keeps the draft of a task deleted elsewhere until it is let go", async () => {
  await openParent();
  await edit({ title: "Typed before it was deleted" });

  mocks.taskMap.delete(parent.id);
  await rerender();

  expect(editor().taskGone).toBe(true);
  expect(editor().taskDraft?.title).toBe("Typed before it was deleted");
  await act(async () => editor().onSaveTask());
  expect(updateCoordinationTask).not.toHaveBeenCalled();
  // Closing asks before the edits go.
  await act(async () => editor().onClose());
  expect(confirm).toHaveBeenCalledOnce();
  expect(editor().taskDraft?.title).toBe("Typed before it was deleted");
});

it("holds the draft still while saving and then shows what was saved", async () => {
  await openParent();
  await edit({ title: "Saved title" });
  let finish!: (value: { ok: true; result: unknown }) => void;
  vi.mocked(updateCoordinationTask).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve as typeof finish;
    }),
  );
  const saved: Task = { ...parent, title: "Saved title", rev: 2 };
  mocks.loadTaskDetail.mockImplementation(async () => {
    mocks.taskMap.set(parent.id, saved);
    return saved;
  });

  let saving!: Promise<void>;
  await act(async () => {
    saving = Promise.resolve(editor().onSaveTask());
  });
  expect(editor().syncBusy).toBe(true);
  await act(async () => {
    finish({ ok: true, result: {} });
    await saving;
  });

  expect(editor().syncBusy).toBe(false);
  expect(editor().taskDraft?.title).toBe("Saved title");
  expect(editor().taskDraft?.base.rev).toBe(2);
  expect(editor().hasTaskUnsaved).toBe(false);
});

it("does not let a late save answer into a drawer opened since", async () => {
  await openParent();
  await edit({ title: "Saved first" });
  let finish!: (value: { ok: true; result: unknown }) => void;
  vi.mocked(updateCoordinationTask).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve as typeof finish;
    }),
  );
  mocks.loadTaskDetail.mockResolvedValue({ ...parent, title: "Saved first", rev: 2 });
  let saving!: Promise<void>;
  await act(async () => {
    saving = Promise.resolve(editor().onSaveTask());
  });
  confirm.mockReturnValue(true);
  await act(async () => editor().onClose());
  await act(async () => mocks.board?.onOpenCreate("planned"));
  await edit({ title: "A new task" });

  await act(async () => {
    finish({ ok: true, result: {} });
    await saving;
  });

  expect(editor().taskEditorMode).toBe("create");
  expect(editor().taskDraft?.title).toBe("A new task");
  expect(editor().error).toBe("");
});

it("says a save went through when it cannot be read back, and waits for a reload", async () => {
  await openParent();
  await edit({ title: "Saved blind" });
  vi.mocked(updateCoordinationTask).mockResolvedValue({ ok: true, result: {} } as never);
  mocks.loadTaskDetail.mockResolvedValue(null);

  await act(async () => editor().onSaveTask());

  // Saved, so nothing is reported as failed; the draft keeps what was saved.
  expect(updateCoordinationTask).toHaveBeenCalledOnce();
  expect(editor().taskStale).toBe("unconfirmed");
  expect(editor().error).toBe("");
  expect(editor().taskDraft?.title).toBe("Saved blind");

  // The reload reads the saved version; the board still has the one before.
  const saved: Task = { ...parent, title: "Saved blind", rev: 2 };
  mocks.loadTaskDetail.mockResolvedValue(saved);
  await act(async () => editor().onResetTask());
  await rerender();
  expect(editor().taskStale).toBe("");
  expect(editor().taskDraft?.title).toBe("Saved blind");
  expect(editor().taskDraft?.base.rev).toBe(2);
  expect(editor().hasTaskUnsaved).toBe(false);
});

it("keeps the draft and its warning when a reload cannot read the task", async () => {
  await openParent();
  await edit({ title: "Mine" });
  vi.mocked(updateCoordinationTask).mockResolvedValue({
    ok: false,
    error: { code: "task_conflict", message: "conflict" },
  });
  mocks.loadTaskDetail.mockResolvedValue(null);
  await act(async () => editor().onSaveTask());
  expect(editor().taskStale).toBe("changed");

  await act(async () => editor().onResetTask());

  expect(editor().taskDraft?.title).toBe("Mine");
  expect(editor().taskStale).toBe("changed");
  expect(editor().error).toContain("could not be loaded");
});

it("deletes only what was confirmed, and says so when that changed", async () => {
  await openParent();
  // Read after the draft was made: the confirmation shows this read.
  mocks.deleteInfoById[parent.id] = {
    allowed: true,
    total: 3,
    reason: "",
    tasksVersion: "v7",
    rev: 4,
  };
  await rerender();
  confirm.mockReturnValue(true);
  vi.mocked(deleteCoordinationTaskConfirmed).mockResolvedValue({
    ok: false,
    error: { code: "task_changed", message: "changed" },
  });

  await act(async () => editor().onDeleteSelectedTask());

  // Three in the subtree: the task and the two below it.
  expect(confirm).toHaveBeenLastCalledWith(expect.stringContaining("and its 2 unexecuted tasks"));
  expect(deleteCoordinationTaskConfirmed).toHaveBeenCalledExactlyOnceWith("g_subtasks", parent.id, {
    rev: 4,
    total: 3,
    tasksVersion: "v7",
  });
  expect(editor().selectedTask?.id).toBe(parent.id);
  expect(editor().error).toContain("nothing was deleted");
});

const drawer = () => document.querySelector('[aria-labelledby="context-task-drawer-title"]');
const lostAnswer = {
  ok: false as const,
  error: { code: "NETWORK_ERROR", message: "Failed to fetch" },
};
const created = {
  ok: true as const,
  result: { changes: [{ op: "task.create", task_id: "T009" }] },
};
const sentIds = () =>
  vi
    .mocked(contextSync)
    .mock.calls.map(([, ops]) => (ops[0] as { client_request_id?: string }).client_request_id);

async function newTask(title: string) {
  await openParent();
  await act(async () => mocks.board?.onOpenCreate("planned"));
  await edit({ title });
}

it("creates a task once when its answer was lost and it is sent again", async () => {
  await newTask("Ship it");
  vi.mocked(contextSync).mockResolvedValueOnce(lostAnswer);
  await act(async () => editor().onSaveTask());
  expect(editor().error).toContain("may already exist");
  expect(drawer()).not.toBeNull();

  vi.mocked(contextSync).mockResolvedValueOnce(created as never);
  await act(async () => editor().onSaveTask());

  // The same id, so the daemon finds the task the first try made.
  expect(sentIds()).toHaveLength(2);
  expect(sentIds()[0]).toBeTruthy();
  expect(sentIds()[1]).toBe(sentIds()[0]);
  expect(confirm).not.toHaveBeenCalled();
  expect(drawer()).toBeNull();
});

it("asks before sending changed content while an earlier try may have been written", async () => {
  await newTask("Ship it");
  vi.mocked(contextSync).mockResolvedValueOnce(lostAnswer);
  await act(async () => editor().onSaveTask());
  // Refused again under the same id: that says nothing about the first try.
  vi.mocked(contextSync).mockResolvedValueOnce({
    ok: false,
    error: { code: "permission_denied", message: "Sign in again" },
  });
  await act(async () => editor().onSaveTask());
  expect(editor().error).toBe("Sign in again");

  await edit({ title: "Ship it today" });
  await act(async () => editor().onSaveTask());
  expect(confirm).toHaveBeenCalledOnce();
  expect(contextSync).toHaveBeenCalledTimes(2);

  confirm.mockReturnValue(true);
  vi.mocked(contextSync).mockResolvedValueOnce(created as never);
  await act(async () => editor().onSaveTask());
  expect(contextSync).toHaveBeenCalledTimes(3);
  expect(sentIds()[2]).not.toBe(sentIds()[0]);
});

it("asks after a failure that may have come after the write", async () => {
  await newTask("Ship it");
  vi.mocked(contextSync).mockResolvedValueOnce({
    ok: false,
    error: { code: "io_error", message: "ledger append failed" },
  });
  await act(async () => editor().onSaveTask());
  expect(editor().error).toBe("ledger append failed");

  await edit({ title: "Ship it today" });
  await act(async () => editor().onSaveTask());
  expect(confirm).toHaveBeenCalledOnce();
  expect(contextSync).toHaveBeenCalledOnce();
});

it("does not ask again for a task a daemon refused outright", async () => {
  await newTask("Ship it");
  vi.mocked(contextSync).mockResolvedValueOnce({
    ok: false,
    error: { code: "invalid_args", message: "bad parent" },
  });
  await act(async () => editor().onSaveTask());
  expect(editor().error).toBe("bad parent");

  await edit({ title: "Ship it today" });
  vi.mocked(contextSync).mockResolvedValueOnce(created as never);
  await act(async () => editor().onSaveTask());
  expect(confirm).not.toHaveBeenCalled();
  expect(drawer()).toBeNull();
});

it("closes the drawer once a task is created, even when the context cannot be read", async () => {
  await newTask("Ship it");
  mocks.syncContext.mockRejectedValue(new Error("offline"));
  vi.mocked(contextSync).mockResolvedValueOnce(created as never);

  await act(async () => editor().onSaveTask());

  expect(drawer()).toBeNull();
  expect(contextSync).toHaveBeenCalledOnce();
});

it("restores an archived card to where it was, fenced on its revision", async () => {
  const archived: Task = { ...first, status: "archived", archived_from: "done", rev: 4 };
  await openParent();
  vi.mocked(restoreCoordinationTask).mockResolvedValue({ ok: true, result: {} } as never);
  mocks.refresh.mockClear();

  await act(async () => mocks.board?.onRestoreTask(archived));

  expect(restoreCoordinationTask).toHaveBeenCalledExactlyOnceWith("g_subtasks", first.id, 4);
  expect(moveCoordinationTask).not.toHaveBeenCalled();
  expect(mocks.refresh).toHaveBeenCalled();
});

async function saveWithLostAnswer(latest: Task | null) {
  await openParent();
  await edit({ title: "Renamed" });
  vi.mocked(updateCoordinationTask).mockResolvedValue(lostAnswer);
  mocks.loadTaskDetail.mockResolvedValue(latest);
  await act(async () => editor().onSaveTask());
}

it("takes a save whose answer was lost as done when the task holds it", async () => {
  await saveWithLostAnswer({ ...parent, title: "Renamed", rev: 2 });
  expect(editor().error).toBe("");
  expect(editor().hasTaskUnsaved).toBe(false);
  expect(editor().taskDraft?.base.rev).toBe(2);
});

it("keeps edits a lost save did not make, and says they were not saved", async () => {
  await saveWithLostAnswer(parent);
  expect(editor().error).toContain("were not saved");
  expect(editor().taskDraft?.title).toBe("Renamed");
  expect(editor().taskStale).toBe("");
});

it("keeps edits after a lost save when the task changed otherwise", async () => {
  await saveWithLostAnswer({ ...parent, notes: "From an agent", rev: 2 });
  expect(editor().taskStale).toBe("changed");
  expect(editor().taskDraft?.title).toBe("Renamed");
});

it("says when it cannot tell whether edits were saved, until the task shows it", async () => {
  await saveWithLostAnswer(null);
  expect(editor().error).toContain("could not be confirmed");
  expect(editor().taskDraft?.title).toBe("Renamed");

  // A later read of the task with those edits settles it.
  mocks.taskMap.set(parent.id, { ...parent, title: "Renamed", rev: 2 });
  await rerender();
  expect(editor().error).toBe("");
  expect(editor().hasTaskUnsaved).toBe(false);
  expect(editor().taskDraft?.base.rev).toBe(2);
});

it("does not decide from a read of the task without its revision", async () => {
  await saveWithLostAnswer({ ...parent, rev: undefined });
  expect(editor().error).toContain("could not be confirmed");
  expect(editor().taskStale).toBe("");
});

it("takes a save as done when only reading the context after it failed", async () => {
  await openParent();
  await edit({ title: "Renamed" });
  vi.mocked(updateCoordinationTask).mockResolvedValue({ ok: true, result: {} } as never);
  mocks.syncContext.mockRejectedValue(new Error("offline"));
  mocks.loadTaskDetail.mockResolvedValue({ ...parent, title: "Renamed", rev: 2 });

  await act(async () => editor().onSaveTask());

  expect(editor().error).toBe("");
  expect(editor().taskDraft?.base.rev).toBe(2);
});

it("reads the board again when a move could not be confirmed", async () => {
  await openParent();
  vi.mocked(moveCoordinationTask).mockResolvedValue(lostAnswer);
  mocks.refresh.mockClear();

  await dragTo(first.id, "active");

  expect(mocks.refresh).toHaveBeenCalled();
  expect(alerts()).toEqual([expect.stringContaining("could not be confirmed")]);
  expect(editor().error).toBe("");
});

it("reads a task again when its deletion could not be confirmed", async () => {
  await openParent();
  mocks.deleteInfoById[parent.id] = {
    allowed: true,
    total: 1,
    reason: "",
    tasksVersion: "v7",
    rev: 1,
  };
  await rerender();
  confirm.mockReturnValue(true);
  vi.mocked(deleteCoordinationTaskConfirmed).mockResolvedValue(lostAnswer);
  mocks.loadTaskDetail.mockClear().mockResolvedValue(null);
  mocks.refresh.mockClear();

  await act(async () => editor().onDeleteSelectedTask());

  expect(editor().error).toContain("could not be confirmed");
  expect(mocks.loadTaskDetail).toHaveBeenCalledWith(parent.id);
  expect(mocks.refresh).toHaveBeenCalled();
});

it("says nothing was deleted when the task could not be checked first", async () => {
  await openParent();
  mocks.deleteInfoById[parent.id] = {
    allowed: true,
    total: 1,
    reason: "",
    tasksVersion: "v7",
    rev: 1,
  };
  await rerender();
  confirm.mockReturnValue(true);
  vi.mocked(deleteCoordinationTaskConfirmed).mockResolvedValue({
    ok: false,
    error: { code: "task_delete_check_unavailable", message: "check failed" },
  });
  mocks.loadTaskDetail.mockClear();
  mocks.refresh.mockClear();

  await act(async () => editor().onDeleteSelectedTask());

  expect(editor().error).toContain("so nothing was deleted");
  expect(mocks.loadTaskDetail).not.toHaveBeenCalled();
  expect(mocks.refresh).not.toHaveBeenCalled();
});

it("reloads a task changed elsewhere once the board has caught up", async () => {
  await openParent();
  await edit({ title: "Mine" });
  vi.mocked(updateCoordinationTask).mockResolvedValue({
    ok: false,
    error: { code: "task_conflict", message: "conflict" },
  });
  mocks.loadTaskDetail.mockResolvedValue(null);
  await act(async () => editor().onSaveTask());
  mocks.refresh.mockClear();
  mocks.loadTaskDetail.mockClear().mockResolvedValue({ ...parent, title: "Theirs", rev: 2 });

  await act(async () => editor().onResetTask());

  // A detail read begun while the board is behind would be dropped.
  const [refreshed] = mocks.refresh.mock.invocationCallOrder;
  const [read] = mocks.loadTaskDetail.mock.invocationCallOrder;
  expect(refreshed).toBeLessThan(read);
  expect(editor().taskDraft?.title).toBe("Theirs");
  expect(editor().taskStale).toBe("");
});

const escape = () =>
  act(async () => {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
  });

it("cancels a keyboard drag with Escape without closing Project Context", async () => {
  root = createRoot(host);
  await act(async () => root.render(modal()));
  await act(async () =>
    mocks.board?.onDragStart({ active: { id: `task:${first.id}` } } as DragStartEvent),
  );

  await escape();
  expect(mocks.close).not.toHaveBeenCalled();

  await act(async () => mocks.board?.onDragCancel());
  await escape();
  expect(mocks.close).toHaveBeenCalledOnce();
});

it("closes only the task editor on Escape", async () => {
  await openParent();

  await escape();

  expect(drawer()).toBeNull();
  expect(mocks.close).not.toHaveBeenCalled();
});

it("asks before other navigation drops unsaved task edits", async () => {
  await openParent();
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
  await edit({ title: "Mine" });
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);

  const proceed = vi.fn();
  requestWorkspaceNavigation(proceed);
  expect(proceed).not.toHaveBeenCalled();
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  finishWorkspaceNavigation(false);

  await act(async () => root.unmount());
  root = createRoot(host);
  await act(async () => root.render(<div />));
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
});

it("shows a task the daemon says is gone as gone, even when it cannot be read again", async () => {
  await openParent();
  await edit({ title: "Mine" });
  vi.mocked(updateCoordinationTask).mockResolvedValue({
    ok: false,
    error: { code: "task_not_found", message: "task not found: T001" },
  });
  mocks.loadTaskDetail.mockClear().mockResolvedValue(null);

  await act(async () => editor().onSaveTask());
  // The board leaving it out renders the editor again.
  await rerender();

  expect(editor().taskGone).toBe(true);
  expect(editor().taskDraft?.title).toBe("Mine");
  expect(mocks.loadTaskDetail).not.toHaveBeenCalled();
});

it("shows a task as gone when its deletion finds it already gone", async () => {
  await openParent();
  mocks.deleteInfoById[parent.id] = {
    allowed: true,
    total: 1,
    reason: "",
    tasksVersion: "v7",
    rev: 1,
  };
  await rerender();
  confirm.mockReturnValue(true);
  vi.mocked(deleteCoordinationTaskConfirmed).mockResolvedValue({
    ok: false,
    error: { code: "task_not_found", message: "task not found: T001" },
  });

  await act(async () => editor().onDeleteSelectedTask());
  await rerender();

  expect(editor().taskGone).toBe(true);
});
