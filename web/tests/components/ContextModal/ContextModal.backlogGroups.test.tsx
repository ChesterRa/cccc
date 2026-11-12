// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { GroupContext, Task } from "../../../src/types";
import type { BacklogPanel as Panel } from "../../../src/components/ContextModal/backlog/BacklogPanel";

type PanelProps = ComponentProps<typeof Panel>;

const mocks = vi.hoisted(() => ({
  panels: [] as unknown[],
  fetchBacklog: vi.fn(),
  fetchTaskById: vi.fn(),
  updateCoordinationTask: vi.fn(),
  moveCoordinationTask: vi.fn(),
  loadTaskDetail: vi.fn(),
  refresh: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, opts?: { defaultValue?: string; [key: string]: unknown }) =>
      (opts?.defaultValue || _key).replace(/\{\{(\w+)\}\}/g, (match, key: string) =>
        opts?.[key] === undefined ? match : String(opts[key]),
      ),
    i18n: { language: "en" },
  }),
}));

vi.mock("../../../src/services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../src/services/api")>()),
  fetchBacklog: (...args: unknown[]) => mocks.fetchBacklog(...args),
  fetchTaskById: (...args: unknown[]) => mocks.fetchTaskById(...args),
  updateCoordinationTask: (...args: unknown[]) => mocks.updateCoordinationTask(...args),
  moveCoordinationTask: (...args: unknown[]) => mocks.moveCoordinationTask(...args),
}));

// The panel itself is tested on its own; here only what it is handed matters.
vi.mock("../../../src/components/ContextModal/backlog/BacklogPanel", () => ({
  BacklogPanel: (props: unknown) => {
    mocks.panels.push(props);
    return <div data-backlog-panel="" />;
  },
}));

vi.mock("../../../src/features/contextModal/useTaskBoardData", () => ({
  useTaskBoardData: () => ({
    columns: {
      planned: { items: [], totalCount: 0, hasMore: false, nextOffset: 0, loading: false },
      active: { items: [], totalCount: 0, hasMore: false, nextOffset: 0, loading: false },
      done: { items: [], totalCount: 0, hasMore: false, nextOffset: 0, loading: false },
      archived: { items: [], totalCount: 0, hasMore: false, nextOffset: 0, loading: false },
    },
    board: { planned: [], active: [], done: [], archived: [] },
    taskIndex: [],
    taskMap: new Map(),
    tasksSummary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
    facets: { assignees: [], unassigned: 0, blocked: 0, waitingUser: 0, pendingHandoffs: 0 },
    error: "",
    loading: false,
    deleteInfoById: {},
    refresh: (...args: unknown[]) => mocks.refresh(...args),
    loadMore: vi.fn(async () => {}),
    loadTaskDetail: (...args: unknown[]) => mocks.loadTaskDetail(...args),
    forgetTask: vi.fn(),
  }),
}));

vi.mock("../../../src/components/modals/settings/CapabilitiesTab", () => ({
  CapabilitiesTab: () => <div />,
}));

import { ContextModal } from "../../../src/components/ContextModal";
import { useGroupStore, useUIStore } from "../../../src/stores";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const context: GroupContext = {
  version: "v1",
  coordination: {
    brief: {
      objective: "",
      current_focus: "",
      constraints: [],
      project_brief: "",
      project_brief_stale: false,
    },
    tasks: [],
  },
  agent_states: [],
  tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
};

const source = { scope_key: "s_one", path: "BACKLOG.md", entry_id: "aaaaaaaaaaaa" };
const task: Task = { id: "T1", title: "Cache tasks", status: "active", rev: 3, source: null };

let host: HTMLDivElement;
let root: Root;

function showGroup(groupId: string) {
  useGroupStore.setState({
    groupDoc: {
      group_id: groupId,
      active_scope_key: "s_one",
      scopes: [{ scope_key: "s_one", url: "/repo" }],
    } as never,
  });
}

async function render(groupId: string) {
  showGroup(groupId);
  await act(async () => {
    root.render(
      <ContextModal
        isOpen
        onClose={vi.fn()}
        groupId={groupId}
        groupTitle={groupId}
        context={context}
        onOpenContext={vi.fn(async () => {})}
        onSyncContext={vi.fn(async () => {})}
        isDark={false}
      />,
    );
  });
}

function latestPanel(): PanelProps {
  const last = mocks.panels.at(-1);
  if (!last) throw new Error("The backlog view was not shown");
  return last as PanelProps;
}

beforeEach(() => {
  mocks.panels.length = 0;
  localStorage.clear();
  localStorage.setItem(
    "cccc-context-task-view",
    JSON.stringify({ g_a: "backlog", g_b: "backlog" }),
  );
  mocks.fetchBacklog
    .mockReset()
    .mockResolvedValue({ ok: false, error: { code: "x", message: "" } });
  mocks.fetchTaskById.mockReset().mockResolvedValue({ ok: true, result: { task } });
  mocks.updateCoordinationTask.mockReset().mockResolvedValue({ ok: true, result: {} });
  mocks.moveCoordinationTask.mockReset().mockResolvedValue({ ok: true, result: {} });
  mocks.loadTaskDetail.mockReset().mockResolvedValue(task);
  mocks.refresh.mockReset().mockResolvedValue(undefined);
  useUIStore.setState({ webReadOnly: false });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useGroupStore.setState({ groupDoc: null });
});

it("lets a late call from the previous Group's backlog view do nothing", async () => {
  await render("g_a");
  const fromA = latestPanel();
  await render("g_b");
  expect(latestPanel()).not.toBe(fromA);

  await act(async () => {
    expect(await fromA.onLinkSource("T1", source)).toEqual({ error: "" });
    expect(await fromA.onArchiveTask(task)).toBe("");
    await fromA.onTaskPromoted("T1");
  });
  expect(mocks.fetchTaskById).not.toHaveBeenCalled();
  expect(mocks.updateCoordinationTask).not.toHaveBeenCalled();
  expect(mocks.moveCoordinationTask).not.toHaveBeenCalled();
  expect(mocks.loadTaskDetail).not.toHaveBeenCalled();
});

it("links and archives in the Group the view belongs to", async () => {
  await render("g_a");
  const panel = latestPanel();
  let linked: unknown;
  await act(async () => {
    mocks.fetchTaskById
      .mockResolvedValueOnce({ ok: true, result: { task } })
      .mockResolvedValueOnce({ ok: true, result: { task: { ...task, source, rev: 4 } } });
    linked = await panel.onLinkSource("T1", source);
  });
  expect(mocks.updateCoordinationTask).toHaveBeenCalledWith("g_a", "T1", { source }, 3);
  expect(linked).toEqual({ task: { ...task, source, rev: 4 } });

  await act(async () => {
    await panel.onArchiveTask({ ...task, source, rev: 4 });
  });
  expect(mocks.moveCoordinationTask).toHaveBeenCalledWith("g_a", "T1", "archived", 4);
});

it("opens a task made from an entry although the board's first read of it is dropped", async () => {
  await render("g_a");
  mocks.refresh.mockClear();
  // The context sync after the write reloads the board meanwhile, so the first read is dropped.
  mocks.loadTaskDetail.mockResolvedValueOnce(null).mockResolvedValueOnce(task);
  await act(async () => {
    await latestPanel().onTaskPromoted("T1");
  });
  expect(mocks.refresh).toHaveBeenCalledTimes(2);
  expect(mocks.loadTaskDetail).toHaveBeenNthCalledWith(1, "T1");
  expect(mocks.loadTaskDetail).toHaveBeenNthCalledWith(2, "T1");
  expect(host.textContent).not.toContain("T1 could not be loaded. Try again.");
  // The drawer is open on it.
  expect(document.querySelector("[data-task-editor]")).not.toBeNull();

  // A task that cannot be read at all is reported, after the same two rounds.
  mocks.refresh.mockClear();
  mocks.loadTaskDetail.mockReset().mockResolvedValue(null);
  await act(async () => {
    await latestPanel().onTaskPromoted("T2");
  });
  expect(mocks.refresh).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain("T2 could not be loaded. Try again.");
});

it("leaves a task linked to another entry as it is, before and after linking", async () => {
  await render("g_a");
  const panel = latestPanel();
  const elsewhere = { ...source, entry_id: "bbbbbbbbbbbb" };
  mocks.fetchTaskById.mockResolvedValueOnce({
    ok: true,
    result: { task: { ...task, source: elsewhere } },
  });
  let first: unknown;
  await act(async () => {
    first = await panel.onLinkSource("T1", source);
  });
  expect(first).toEqual({
    error: "T1 is already linked to another entry, so it was left as it is.",
  });
  expect(mocks.updateCoordinationTask).not.toHaveBeenCalled();

  // Someone else linked it between the write and the read that follows it.
  mocks.fetchTaskById
    .mockResolvedValueOnce({ ok: true, result: { task } })
    .mockResolvedValueOnce({ ok: true, result: { task: { ...task, source: elsewhere, rev: 5 } } });
  let second: unknown;
  await act(async () => {
    second = await panel.onLinkSource("T1", source);
  });
  expect(second).toEqual({
    error: "T1 is already linked to another entry, so it was left as it is.",
  });
});
