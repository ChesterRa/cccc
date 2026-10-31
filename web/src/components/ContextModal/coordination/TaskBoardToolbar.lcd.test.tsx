// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { setUiStylePrefs } from "../../../utils/uiStyle";
import { createContextModalUi } from "../ui";
import { TaskBoardToolbar } from "./TaskBoardToolbar";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof TaskBoardToolbar>;

beforeEach(() => {
  setUiStylePrefs({ style: "lcd" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  props = {
    tr: (key, fallback) => fallback || key,
    ui: createContextModalUi(false),
    syncBusy: false,
    taskQuery: "",
    assigneeFilter: "__all__",
    assigneeOptions: ["worker"],
    taskFilter: "all",
    tasksSummary: { total: 5 },
    attentionCounts: { blocked: 1, waitingUser: 0, pendingHandoffs: 0 },
    unassignedCount: 0,
    hasArchivedTasks: true,
    archivedExpanded: false,
    onTaskQueryChange: vi.fn(),
    onAssigneeFilterChange: vi.fn(),
    onTaskFilterChange: vi.fn(),
    onClearFilters: vi.fn(),
    onArchivedExpandedChange: vi.fn(),
  };
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setUiStylePrefs({ style: "classic" });
});

function render(extra: Partial<ComponentProps<typeof TaskBoardToolbar>> = {}) {
  act(() => root.render(<TaskBoardToolbar {...props} {...extra} />));
}

it("leads the controls with Clear and keeps the board's own key on the toolbar row", () => {
  render({ taskQuery: "report", actions: <button type="button">New</button> });
  const controls = host.querySelector("[data-taskboard-controls]")!;
  expect(controls.firstElementChild?.hasAttribute("data-taskboard-clear")).toBe(true);
  act(() => controls.querySelector<HTMLButtonElement>("[data-taskboard-clear]")!.click());
  expect(props.onClearFilters).toHaveBeenCalledTimes(1);
  const row = host.querySelector("[data-taskboard-toolbar]")!;
  expect(row.lastElementChild?.textContent).toBe("New");

  render();
  expect(host.querySelector("[data-taskboard-clear]")).toBeNull();
});

it("shows archived tasks with a pressed key instead of a switch", () => {
  render();
  const archive = host.querySelector<HTMLButtonElement>("[data-taskboard-archive]")!;
  expect(archive.getAttribute("aria-pressed")).toBe("false");
  expect(host.querySelector('[role="switch"], input[type="checkbox"]')).toBeNull();
  act(() => archive.click());
  expect(props.onArchivedExpandedChange).toHaveBeenCalledWith(true);
});

it("pins the column jump and the status together, and nothing when both are absent", () => {
  render({ jump: <nav data-testid="jump" />, matchingCount: 2 });
  const layer = host.querySelector("[data-taskboard-jump-layer]")!;
  expect(layer.querySelector('[data-testid="jump"]')).not.toBeNull();
  expect(layer.querySelector('[role="status"]')?.textContent).toBe("{{count}} matching tasks");
  expect(layer.closest("[data-taskboard-sticky]")).not.toBeNull();

  render();
  expect(host.querySelector("[data-taskboard-jump-layer]")).toBeNull();
});

it("pads the scroller by the pinned layer's measured height until it goes", () => {
  const heights = { sticky: 111.375, jump: 40 };
  const rect = vi
    .spyOn(Element.prototype, "getBoundingClientRect")
    .mockImplementation(function (this: Element) {
      const height = this.hasAttribute("data-taskboard-sticky")
        ? heights.sticky
        : this.hasAttribute("data-taskboard-jump-layer")
          ? heights.jump
          : 0;
      return new DOMRect(0, 0, 0, height);
    });
  try {
    const inScroller = (key: string) =>
      act(() =>
        root.render(
          <div key={key} data-context-scroll="">
            <TaskBoardToolbar {...props} jump={<nav />} />
          </div>,
        ),
      );
    const pinned = (scroll: HTMLElement) =>
      scroll.style.getPropertyValue("--taskboard-pinned-height");

    inScroller("desktop");
    const desktop = host.querySelector<HTMLElement>("[data-context-scroll]")!;
    expect(pinned(desktop)).toBe("112px");

    // A phone pins only the jump layer; the outer layer has no box there.
    heights.sticky = 0;
    heights.jump = 52.5;
    inScroller("phone");
    expect(pinned(desktop)).toBe("");
    const phone = host.querySelector<HTMLElement>("[data-context-scroll]")!;
    expect(pinned(phone)).toBe("53px");

    act(() => setUiStylePrefs({ style: "classic" }));
    expect(pinned(phone)).toBe("");
  } finally {
    rect.mockRestore();
  }
});
