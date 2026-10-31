// @vitest-environment happy-dom
import { DndContext } from "@dnd-kit/core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Task } from "../../../types";
import { setUiStylePrefs } from "../../../utils/uiStyle";
import { createContextModalUi } from "../ui";
import { TaskCard } from "./TaskCard";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ i18n: { language: "en" } }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const tr = (_key: string, fallback: string, values?: Record<string, unknown>) =>
  fallback.replace(/\{\{(\w+)\}\}/g, (_match, key: string) => String(values?.[key] ?? ""));

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setUiStylePrefs({ style: "lcd" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setUiStylePrefs({ style: "classic" });
});

function render(task: Task, searchQuery = "", handlers: Partial<Record<string, () => void>> = {}) {
  act(() =>
    root.render(
      <DndContext>
        <TaskCard
          task={task}
          tr={tr}
          ui={createContextModalUi(false)}
          syncBusy={false}
          searchQuery={searchQuery}
          selected={false}
          onSelectTask={vi.fn()}
          onMoveTaskToStatus={handlers.move ?? vi.fn()}
          onRestoreTask={handlers.restore ?? vi.fn()}
        />
      </DndContext>,
    ),
  );
  return host.querySelector<HTMLElement>("[data-task-row]")!;
}

it("marks a match in the whole title and quotes nothing the row already shows", () => {
  const task: Task = {
    id: "T7",
    title: "Prepare the acceptance environment",
    outcome: "Acceptance notes are filed",
    status: "active",
  };
  let row = render(task, "acceptance");
  expect(row.hasAttribute("data-searching")).toBe(true);
  expect(row.querySelector("[data-task-title] mark")?.textContent).toBe("acceptance");
  expect(row.querySelector("[data-task-excerpt]")).toBeNull();
  // The summary stays, since the match is already in sight.
  expect(row.querySelector("[data-task-summary]")?.textContent).toBe("Acceptance notes are filed");

  row = render(task, "t7");
  expect(row.querySelector("mark")).toBeNull();
  expect(row.querySelector("[data-task-excerpt]")).toBeNull();
});

it("quotes another matched field by name in place of the summary", () => {
  const assignee = "reviewer-with-a-very-long-name-that-a-narrow-row-would-cut";
  const row = render(
    { id: "T8", title: "Check rounding", outcome: "Totals agree", assignee, status: "active" },
    "narrow-row",
  );
  const excerpt = row.querySelector("[data-task-excerpt]");
  expect(excerpt?.textContent).toContain("Assignee:");
  expect(excerpt?.querySelector("mark")?.textContent).toBe("narrow-row");
  expect(row.querySelector("[data-task-summary]")).toBeNull();

  const priority = render({ id: "T9", title: "Check rounding", priority: "high" }, "high");
  expect(priority.querySelector("[data-task-excerpt]")?.textContent).toContain("Priority:");
});

it("keeps the row's keys beside its text and restores an archived task", () => {
  const restore = vi.fn();
  const move = vi.fn();
  const row = render(
    { id: "T3", title: "Old task", status: "archived", archived_from: "done" },
    "",
    { restore, move },
  );
  const actions = row.querySelector("[data-task-actions]")!;
  expect(actions.parentElement).toBe(row);
  expect(row.querySelector("[data-task-meta] [data-task-actions]")).toBeNull();
  expect(actions.querySelector("[data-task-drag]")).not.toBeNull();
  act(() => actions.querySelector<HTMLButtonElement>("[data-task-quick]")!.click());
  expect(restore).toHaveBeenCalledTimes(1);
  expect(move).not.toHaveBeenCalled();
});

it("writes attention as notes, with waiting on the user as the one tag", () => {
  const row = render({
    id: "T4",
    title: "Confirm the discount rule",
    status: "active",
    assignee: "codex",
    waiting_on: "user",
    blocked_by: ["T2"],
    handoff_to: "claude",
    priority: "high",
    parent_id: "T1",
  });
  const note = (kind: string) => row.querySelector<HTMLElement>(`[data-task-note="${kind}"]`);
  expect(note("waiting-user")?.textContent).not.toMatch(/^\[/);
  expect(note("blocked")?.textContent).toBe("[Blocked: T2]");
  expect(note("handoff")?.textContent).toBe("[→claude]");
  expect(note("handoff")?.title).toBe("Handoff → claude");
  expect(note("assignee")?.textContent).toBe("codex");
  // Attention leads, so the line's tail, which the hover keys may cover, is detail.
  expect(
    [...row.querySelectorAll("[data-task-note]")].map((el) => el.getAttribute("data-task-note")),
  ).toEqual(["assignee", "blocked", "waiting-user", "handoff", "priority", "parent"]);
});

it("keeps the classic drag handle free of the LCD hook", () => {
  setUiStylePrefs({ style: "classic" });
  render({ id: "T5", title: "Classic card", status: "planned" });
  const handle = host.querySelector('button[title="Drag task"]');
  expect(handle).not.toBeNull();
  expect(handle?.hasAttribute("data-task-drag")).toBe(false);
  expect(host.querySelector("[data-task-row]")).toBeNull();
});
