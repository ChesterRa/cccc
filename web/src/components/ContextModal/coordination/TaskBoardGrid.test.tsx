// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { createTaskColumnPages } from "../../../features/contextModal/taskBoardData";
import type { Task } from "../../../types";
import { createContextModalUi } from "../ui";
import { TaskBoardGrid } from "./TaskBoardGrid";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("keeps focus on its task when the column order flips", () => {
  const planned: Task = { id: "T1", title: "Write the plan", status: "planned" };
  const active: Task = { id: "T2", title: "Fix the board", status: "active" };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const props: ComponentProps<typeof TaskBoardGrid> = {
    tr: (_key, fallback) => fallback,
    ui: createContextModalUi(false),
    syncBusy: false,
    searchQuery: "",
    archivedExpanded: false,
    statuses: ["planned", "active", "done"],
    board: { planned: [planned], active: [active], done: [], archived: [] },
    pages: createTaskColumnPages(),
    taskMap: new Map([
      [planned.id, planned],
      [active.id, active],
    ]),
    subtaskProgress: new Map(),
    selectedTaskId: "",
    dragTaskId: "",
    sensors: [],
    onDragStart: vi.fn(),
    onDragEnd: vi.fn(),
    onDragCancel: vi.fn(),
    onSelectTask: vi.fn(),
    onMoveTaskToStatus: vi.fn(),
    onRestoreTask: vi.fn(),
    onLoadMore: vi.fn(),
  };
  const columns = () =>
    [...host.querySelectorAll("section[aria-label]")].map((column) =>
      column.getAttribute("aria-label"),
    );
  // Chrome drops the focus inside a node that is moved (checked in Chrome 153);
  // happy-dom keeps it, so do what Chrome does.
  const { insertBefore, appendChild } = Node.prototype;
  const blurIfMoved = (node: Node) => {
    const focused = document.activeElement;
    if (node.isConnected && focused instanceof HTMLElement && node.contains(focused)) {
      focused.blur();
    }
  };
  Node.prototype.insertBefore = function <T extends Node>(this: Node, node: T, child: Node | null) {
    blurIfMoved(node);
    return insertBefore.call(this, node, child) as T;
  };
  Node.prototype.appendChild = function <T extends Node>(this: Node, node: T) {
    blurIfMoved(node);
    return appendChild.call(this, node) as T;
  };
  try {
    act(() => root.render(<TaskBoardGrid {...props} />));
    expect(columns()).toEqual(["Planned", "Active", "Done"]);

    // Planned is the column that moves when Active is put before it.
    const plannedTask = host.querySelector<HTMLElement>(
      'section[aria-label="Planned"] [tabindex="0"]',
    )!;
    plannedTask.focus();
    act(() => root.render(<TaskBoardGrid {...props} statuses={["active", "planned", "done"]} />));
    expect(columns()).toEqual(["Active", "Planned", "Done"]);
    expect(document.activeElement).toBe(plannedTask);
    // The node moved with its column rather than being reused for another one.
    expect(plannedTask.closest("section")?.getAttribute("aria-label")).toBe("Planned");

    act(() =>
      root.render(
        <TaskBoardGrid
          {...props}
          archivedExpanded
          statuses={["active", "planned", "done", "archived"]}
        />,
      ),
    );
    expect(columns()).toEqual(["Active", "Planned", "Done", "Archived"]);

    const activeTask = host.querySelector<HTMLElement>(
      'section[aria-label="Active"] [tabindex="0"]',
    )!;
    activeTask.focus();
    act(() =>
      root.render(
        <TaskBoardGrid
          {...props}
          archivedExpanded
          statuses={["planned", "active", "done", "archived"]}
        />,
      ),
    );
    expect(columns()).toEqual(["Planned", "Active", "Done", "Archived"]);
    expect(document.activeElement).toBe(activeTask);
    expect(activeTask.closest("section")?.getAttribute("aria-label")).toBe("Active");
  } finally {
    Object.assign(Node.prototype, { insertBefore, appendChild });
    act(() => root.unmount());
    host.remove();
  }
});
