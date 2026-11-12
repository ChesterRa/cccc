// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { BacklogSnapshot, GroupContext } from "../../../src/types";

const mocks = vi.hoisted(() => ({
  fetchBacklog: vi.fn(),
  backlogOperation: vi.fn(),
  confirm: vi.fn(() => false),
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
  backlogOperation: (...args: unknown[]) => mocks.backlogOperation(...args),
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
    refresh: vi.fn(async () => {}),
    loadMore: vi.fn(async () => {}),
    loadTaskDetail: vi.fn(async () => null),
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

const backlog: BacklogSnapshot = {
  configured: true,
  scope_key: "s_one",
  scope_url: "/repo",
  path: "BACKLOG.md",
  exists: true,
  sha256: "a".repeat(64),
  git: { repository: true, tracked: false, ignored: true },
  problems: [],
  sections: [{ name: "Now", line: 1, closed: false, entries: [] }],
};

let host: HTMLDivElement;
let root: Root;
const onClose = vi.fn();

async function render(groupId = "g_a") {
  await act(async () => {
    root.render(
      <ContextModal
        isOpen
        onClose={onClose}
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

function radio(label: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find(
    (item) => item.textContent?.trim() === label,
  );
}

beforeEach(() => {
  localStorage.clear();
  mocks.fetchBacklog.mockReset().mockResolvedValue({ ok: true, result: backlog });
  mocks.backlogOperation.mockReset();
  mocks.confirm.mockReset().mockReturnValue(false);
  onClose.mockReset();
  Object.defineProperty(window, "confirm", { value: mocks.confirm, configurable: true });
  useUIStore.setState({ webReadOnly: false });
  useGroupStore.setState({
    groupDoc: {
      group_id: "g_a",
      active_scope_key: "s_one",
      scopes: [{ scope_key: "s_one", url: "/repo" }],
    } as never,
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useGroupStore.setState({ groupDoc: null });
});

it("reads the backlog once on opening and shows it beside the board when chosen", async () => {
  await render();
  expect(mocks.fetchBacklog).toHaveBeenCalledTimes(1);
  expect(mocks.fetchBacklog).toHaveBeenCalledWith("g_a", "s_one", "/repo");
  expect(radio("Tasks")?.getAttribute("aria-checked")).toBe("true");
  expect(host.querySelector("[data-backlog-panel]")).toBeNull();

  await act(async () => radio("Backlog")?.click());
  expect(host.querySelector("[data-backlog-panel]")).not.toBeNull();
  expect(host.textContent).toContain("BACKLOG.md");
  expect(mocks.fetchBacklog).toHaveBeenCalledTimes(2);
  // Each Group keeps the view it was left on.
  expect(JSON.parse(localStorage.getItem("cccc-context-task-view") || "{}")).toEqual({
    g_a: "backlog",
  });

  await render("g_b");
  expect(radio("Tasks")?.getAttribute("aria-checked")).toBe("true");
  await render("g_a");
  expect(radio("Backlog")?.getAttribute("aria-checked")).toBe("true");
});

it("offers no backlog on the read-only page, even where this browser last showed it", async () => {
  localStorage.setItem("cccc-context-task-view", JSON.stringify({ g_a: "backlog" }));
  useUIStore.setState({ webReadOnly: true });
  await render();
  expect(host.querySelector('[role="radiogroup"]')).toBeNull();
  // The board, not a backlog view that has nothing to read.
  expect(host.querySelector("[data-goal-strip]")).not.toBeNull();
  expect(mocks.fetchBacklog).not.toHaveBeenCalled();
});

it("asks before closing over an unsaved backlog draft", async () => {
  await render();
  await act(async () => radio("Backlog")?.click());
  const add = [...host.querySelectorAll("button")].find((item) => item.textContent === "Entry");
  await act(async () => add?.click());
  const title = host.querySelector<HTMLInputElement>("[data-backlog-panel] form input");
  if (!title) throw new Error("Missing title");
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(title, "Idea");
    title.dispatchEvent(new Event("input", { bubbles: true }));
  });

  await act(async () =>
    host.querySelector<HTMLButtonElement>('button[aria-label="Close context modal"]')?.click(),
  );
  expect(mocks.confirm).toHaveBeenCalledWith(
    "You have an unsaved backlog draft. Discard it and close?",
  );
  expect(onClose).not.toHaveBeenCalled();
});
