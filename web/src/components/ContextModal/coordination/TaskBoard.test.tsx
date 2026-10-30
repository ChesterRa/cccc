// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { createTaskColumnPages } from "../../../features/contextModal/taskBoardData";
import { createContextModalUi } from "../ui";
import { TaskBoard } from "./TaskBoard";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("distinguishes an empty board from a filtered, loading, or failed one", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const clear = vi.fn();
  const retry = vi.fn();
  const props: ComponentProps<typeof TaskBoard> = {
    tr: (_key, fallback, values) =>
      fallback.replace(/\{\{(\w+)\}\}/g, (_match, key: string) => String(values?.[key] ?? "")),
    ui: createContextModalUi(false),
    syncBusy: false,
    taskQuery: "",
    assigneeFilter: "__all__",
    assigneeOptions: [],
    taskFilter: "all",
    tasksSummary: { total: 0 },
    attentionCounts: { blocked: 0, waitingUser: 0, pendingHandoffs: 0 },
    unassignedCount: 0,
    hasArchivedTasks: false,
    archivedExpanded: false,
    hasVisibleTasks: false,
    hiddenArchivedMatches: 0,
    filteredBoard: { planned: [], active: [], done: [], archived: [] },
    columnPages: createTaskColumnPages(),
    taskLoading: false,
    taskLoadError: "",
    taskMap: new Map(),
    subtaskProgress: new Map(),
    selectedTaskId: "",
    dragTaskId: "",
    sensors: [],
    onTaskQueryChange: vi.fn(),
    onAssigneeFilterChange: vi.fn(),
    onTaskFilterChange: vi.fn(),
    onClearFilters: clear,
    onArchivedExpandedChange: vi.fn(),
    onOpenCreate: vi.fn(),
    onDragStart: vi.fn(),
    onDragEnd: vi.fn(),
    onDragCancel: vi.fn(),
    onSelectTask: vi.fn(),
    onMoveTaskToStatus: vi.fn(),
    onRestoreTask: vi.fn(),
    onLoadMore: vi.fn(),
    onRetryLoad: retry,
  };
  const render = (patch: Partial<typeof props> = {}) =>
    act(() => root.render(<TaskBoard {...props} {...patch} />));
  try {
    render();
    expect(host.textContent).toContain("No tasks yet");
    expect(host.querySelector("[data-task-column]")).toBeNull();
    render({ taskQuery: "missing", tasksSummary: { total: 2 } });
    expect(host.textContent).toContain("No tasks match the current filters");
    expect(host.querySelector("[data-task-column]")).toBeNull();
    const clearButtons = [...host.querySelectorAll("button")].filter(
      (button) => button.textContent === "Clear filters",
    );
    act(() => clearButtons.at(-1)!.click());
    expect(clear).toHaveBeenCalledOnce();
    render({ taskLoading: true, hasVisibleTasks: true });
    expect(host.textContent).toContain("Loading tasks…");
    expect(host.textContent).not.toContain("No tasks yet");
    expect(host.querySelector("[data-task-column]")).toBeNull();
    render({ taskLoadError: "Connection failed" });
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Connection failed");
    expect(host.querySelector("[data-task-column]")).toBeNull();
    act(() => host.querySelector<HTMLButtonElement>('[role="alert"] button')!.click());
    expect(retry).toHaveBeenCalledOnce();
    render({ hasVisibleTasks: true, tasksSummary: { total: 1 } });
    expect(host.querySelector('[data-task-column="planned"]')?.getAttribute("aria-label")).toBe(
      "Planned",
    );
    expect(host.querySelector('[data-task-column="active"]')?.getAttribute("aria-label")).toBe(
      "Active",
    );
    render({ taskQuery: "review", matchingCount: 3 });
    expect(host.textContent).toContain("3 matching tasks");
    render({ taskQuery: "review", matchingCount: 3, taskLoading: true });
    expect(host.textContent).not.toContain("matching tasks");
    render({ taskQuery: "review", matchingCount: 3, taskLoadError: "offline" });
    expect(host.textContent).not.toContain("matching tasks");
    render({ taskQuery: "   ", matchingCount: 3 });
    expect(host.textContent).not.toContain("matching tasks");
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});

it("quotes matches for the search being requested, not the text still being typed", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const task = {
    id: "T5",
    title: "Speed up deploys",
    status: "active",
    notes: "Warm the cache first.",
  };
  const board = { planned: [], active: [task], done: [], archived: [] };
  const props: ComponentProps<typeof TaskBoard> = {
    tr: (_key, fallback, values) =>
      fallback.replace(/\{\{(\w+)\}\}/g, (_match, key: string) => String(values?.[key] ?? "")),
    ui: createContextModalUi(false),
    syncBusy: false,
    taskQuery: "",
    assigneeFilter: "__all__",
    assigneeOptions: [],
    taskFilter: "all",
    tasksSummary: { total: 1 },
    attentionCounts: { blocked: 0, waitingUser: 0, pendingHandoffs: 0 },
    unassignedCount: 0,
    hasArchivedTasks: false,
    archivedExpanded: false,
    hasVisibleTasks: true,
    hiddenArchivedMatches: 0,
    filteredBoard: board,
    columnPages: createTaskColumnPages(),
    taskLoading: false,
    taskLoadError: "",
    taskMap: new Map([[task.id, task]]),
    subtaskProgress: new Map(),
    selectedTaskId: "",
    dragTaskId: "",
    sensors: [],
    onTaskQueryChange: vi.fn(),
    onAssigneeFilterChange: vi.fn(),
    onTaskFilterChange: vi.fn(),
    onClearFilters: vi.fn(),
    onArchivedExpandedChange: vi.fn(),
    onOpenCreate: vi.fn(),
    onDragStart: vi.fn(),
    onDragEnd: vi.fn(),
    onDragCancel: vi.fn(),
    onSelectTask: vi.fn(),
    onMoveTaskToStatus: vi.fn(),
    onRestoreTask: vi.fn(),
    onLoadMore: vi.fn(),
    onRetryLoad: vi.fn(),
  };
  const render = (patch: Partial<typeof props>) =>
    act(() => root.render(<TaskBoard {...props} {...patch} />));
  try {
    render({ taskQuery: "cache", searchQuery: "" });
    expect(host.querySelector("mark")).toBeNull();
    render({ taskQuery: "cach", searchQuery: "cache" });
    expect(host.querySelector("mark")?.textContent).toBe("cache");
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});
