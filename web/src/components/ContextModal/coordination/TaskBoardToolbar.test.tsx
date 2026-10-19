// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { createContextModalUi } from "../ui";
import { TaskBoardToolbar } from "./TaskBoardToolbar";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("offers Clear filters only when a filter is active and retains its existing action", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const props: ComponentProps<typeof TaskBoardToolbar> = {
    tr: (key, fallback) => fallback || key,
    ui: createContextModalUi(false),
    syncBusy: false,
    taskQuery: "",
    assigneeFilter: "__all__",
    assigneeOptions: ["worker"],
    taskFilter: "all",
    tasksSummary: { total: 5 },
    attentionCounts: { blocked: 1, waitingUser: 1, pendingHandoffs: 0 },
    unassignedCount: 1,
    hasArchivedTasks: false,
    archivedExpanded: false,
    onTaskQueryChange: vi.fn(),
    onAssigneeFilterChange: vi.fn(),
    onTaskFilterChange: vi.fn(),
    onClearFilters: vi.fn(),
    onArchivedExpandedChange: vi.fn(),
  };
  const clear = () =>
    [...host.querySelectorAll("button")].find((b) => b.textContent === "Clear filters");
  try {
    act(() => root.render(<TaskBoardToolbar {...props} />));
    expect(clear()).toBeUndefined();
    for (const filter of [
      { taskQuery: "report" },
      { assigneeFilter: "worker" },
      { taskFilter: "blocked" as const },
    ]) {
      act(() => root.render(<TaskBoardToolbar {...props} {...filter} />));
      expect(clear()?.disabled).toBe(false);
      act(() => clear()!.click());
    }
    expect(props.onClearFilters).toHaveBeenCalledTimes(3);
    act(() => root.render(<TaskBoardToolbar {...props} />));
    expect(clear()).toBeUndefined();
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});
