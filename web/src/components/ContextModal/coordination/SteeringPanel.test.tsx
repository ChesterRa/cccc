// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { createContextModalUi } from "../ui";
import { SteeringPanel } from "./SteeringPanel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof SteeringPanel>;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  props = {
    tr: (key, fallback) => fallback || key,
    ui: createContextModalUi(false),
    brief: {
      objective: "Ship the maintenance release",
      current_focus: "Review the layout",
      project_brief: "Full working summary",
      constraints: ["Preserve runtime history"],
      project_brief_stale: true,
      updated_at: "2026-10-19T04:00:00Z",
    },
    tasksSummary: { active: 2 },
    attentionCounts: { blocked: 1, waitingUser: 1, pendingHandoffs: 0 },
    unassignedCount: 0,
    steeringTab: "summary",
    editingBrief: false,
    briefDraft: {
      objective: "Draft objective",
      currentFocus: "Draft focus",
      projectBrief: "Unsaved summary",
      constraints: "Unsaved constraint",
      projectBriefStale: false,
    },
    syncBusy: false,
    activityBusyKind: null,
    activityError: "",
    recentDecisions: [],
    recentHandoffs: [],
    decisionDraft: { summary: "", taskId: "" },
    handoffDraft: { summary: "", taskId: "" },
    activeTaskOptions: [],
    projectPanel: <div>Project reference</div>,
    onOpenSteeringTab: vi.fn(),
    onStartBriefEdit: vi.fn(),
    onCancelBriefEdit: vi.fn(),
    onSaveBrief: vi.fn(),
    onBriefDraftChange: vi.fn(),
    onDecisionDraftChange: vi.fn(),
    onHandoffDraftChange: vi.fn(),
    onAddCoordinationNote: vi.fn(),
  };
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function render() {
  act(() => root.render(<SteeringPanel {...props} />));
}

function button(label: string) {
  const found = [...host.querySelectorAll("button")].find((el) => el.textContent === label);
  if (!found) throw new Error(`Missing button: ${label}`);
  return found;
}

it("keeps orientation and warnings visible while the summary opens on demand", () => {
  render();
  expect(host.textContent).toContain("Ship the maintenance release");
  expect(host.textContent).toContain("Review the layout");
  expect(host.textContent).toContain("Blocked");
  expect(host.textContent).toContain("Waiting user");
  expect(host.textContent).toContain("Summary needs refresh");
  expect(host.textContent).toContain("Updated");
  expect(button("Summary").getAttribute("aria-expanded")).toBe("false");
  expect(host.textContent).not.toContain("Full working summary");

  act(() => button("Summary").click());
  expect(button("Summary").getAttribute("aria-expanded")).toBe("true");
  expect(host.textContent).toContain("Full working summary");
  expect(host.textContent).toContain("Preserve runtime history");
  const controlled = button("Summary").getAttribute("aria-controls");
  expect(document.getElementById(controlled!)?.textContent).toContain("Full working summary");
  act(() => button("Summary").click());
  expect(host.textContent).not.toContain("Full working summary");
  expect(props.onOpenSteeringTab).not.toHaveBeenCalled();
  expect(props.onSaveBrief).not.toHaveBeenCalled();
});

it("shows and retains the editor even when an external warning opens it", () => {
  render();
  act(() => button("Summary needs refresh").click());
  expect(props.onStartBriefEdit).toHaveBeenCalledOnce();
  props.editingBrief = true;
  render();
  const draft = [...host.querySelectorAll("textarea")].find((el) => el.value === "Unsaved summary");
  expect(draft).toBeDefined();
  act(() => button("Summary").click());
  expect(host.contains(draft!)).toBe(true);
  expect(button("Summary").getAttribute("aria-expanded")).toBe("true");
  act(() => button("Save summary").click());
  expect(props.onSaveBrief).toHaveBeenCalledOnce();
});

it("uses the existing PROJECT.md and log navigation and opens Summary on return", () => {
  render();
  act(() => button("PROJECT.md").click());
  expect(props.onOpenSteeringTab).toHaveBeenLastCalledWith("project");
  props.steeringTab = "project";
  render();
  expect(host.textContent).toContain("Project reference");
  act(() => button("Summary").click());
  expect(props.onOpenSteeringTab).toHaveBeenLastCalledWith("summary");
  props.steeringTab = "summary";
  render();
  expect(host.textContent).toContain("Full working summary");
  act(() => button("Activity").click());
  expect(props.onOpenSteeringTab).toHaveBeenLastCalledWith("log");
});
