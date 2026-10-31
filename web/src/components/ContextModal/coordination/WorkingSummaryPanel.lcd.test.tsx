// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { setUiStylePrefs } from "../../../utils/uiStyle";
import { createContextModalUi } from "../ui";
import { WorkingSummaryPanel } from "./WorkingSummaryPanel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof WorkingSummaryPanel>;

beforeEach(() => {
  setUiStylePrefs({ style: "lcd" });
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
      project_brief_stale: false,
      updated_at: "2026-10-19T04:00:00Z",
    },
    tasksSummary: { active: 2 },
    attentionCounts: { blocked: 0, waitingUser: 0, pendingHandoffs: 0 },
    unassignedCount: 0,
    editingBrief: false,
    briefDraft: {
      objective: "Draft objective",
      currentFocus: "Draft focus",
      projectBrief: "Unsaved summary",
      constraints: "",
      projectBriefStale: false,
    },
    syncBusy: false,
    onStartBriefEdit: vi.fn(),
    onCancelBriefEdit: vi.fn(),
    onSaveBrief: vi.fn(),
    onBriefDraftChange: vi.fn(),
  };
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setUiStylePrefs({ style: "classic" });
});

function render() {
  act(() => root.render(<WorkingSummaryPanel {...props} />));
}

const buttons = (label: string) =>
  [...host.querySelectorAll("button")].filter((el) => el.textContent === label);

it("keeps the objective, the focus and both keys in one strip", () => {
  render();
  const strip = host.querySelector("[data-goal-strip]")!;
  expect(strip.querySelector("[data-goal-objective]")?.textContent).toBe(
    "Ship the maintenance release",
  );
  expect(strip.querySelector("[data-goal-focus]")?.textContent).toBe("Review the layout");
  expect(strip.querySelector("[data-goal-summary-key]")?.getAttribute("aria-expanded")).toBe(
    "false",
  );
  expect(strip.querySelector("[data-goal-edit-key]")).not.toBeNull();
  // The brief stays closed until asked for.
  expect(host.textContent).not.toContain("Full working summary");
});

it("opens the whole brief from the summary key and offers Edit only once", () => {
  render();
  const summaryKey = host.querySelector<HTMLButtonElement>("[data-goal-summary-key]")!;
  act(() => summaryKey.click());
  expect(summaryKey.getAttribute("aria-expanded")).toBe("true");
  expect(host.querySelector("[data-goal-strip]")?.hasAttribute("data-expanded")).toBe(true);
  // The stylesheet wraps long links in these two.
  expect(host.querySelector("[data-goal-brief]")?.textContent).toBe("Full working summary");
  expect(host.querySelector("[data-goal-constraint]")?.textContent).toBe(
    "Preserve runtime history",
  );
  expect(buttons("Edit")).toHaveLength(1);

  act(() => buttons("Edit")[0].click());
  expect(props.onStartBriefEdit).toHaveBeenCalledTimes(1);
});

it("hides the strip's Edit while editing and keeps Cancel and Save in the brief", () => {
  props.editingBrief = true;
  render();
  expect(host.querySelector("[data-goal-edit-key]")).toBeNull();
  expect(buttons("Edit")).toHaveLength(0);
  expect(host.querySelector("[data-goal-unsaved]")).not.toBeNull();
  act(() => buttons("Save summary")[0].click());
  expect(props.onSaveBrief).toHaveBeenCalledTimes(1);
  expect(buttons("Cancel")).toHaveLength(1);
});

it("notes a stale summary as a way into the editor", () => {
  props.brief = { ...props.brief!, project_brief_stale: true };
  render();
  act(() => host.querySelector<HTMLButtonElement>("[data-goal-stale]")!.click());
  expect(props.onStartBriefEdit).toHaveBeenCalledTimes(1);
});
