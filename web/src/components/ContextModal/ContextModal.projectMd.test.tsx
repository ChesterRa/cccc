// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupContext, ProjectMdInfo } from "../../types";

const mocks = vi.hoisted(() => ({
  apiJson: vi.fn(),
  addCoordinationNote: vi.fn(),
  contextSync: vi.fn(),
  deleteCoordinationTask: vi.fn(),
  updateCoordinationBrief: vi.fn(),
  updateCoordinationTask: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, opts?: { defaultValue?: string }) => opts?.defaultValue || _key,
  }),
}));

vi.mock("../../services/api", () => ({
  apiJson: (...args: unknown[]) => mocks.apiJson(...args),
  addCoordinationNote: (...args: unknown[]) => mocks.addCoordinationNote(...args),
  contextSync: (...args: unknown[]) => mocks.contextSync(...args),
  deleteCoordinationTask: (...args: unknown[]) => mocks.deleteCoordinationTask(...args),
  updateCoordinationBrief: (...args: unknown[]) => mocks.updateCoordinationBrief(...args),
  updateCoordinationTask: (...args: unknown[]) => mocks.updateCoordinationTask(...args),
}));

vi.mock("../../features/contextModal/useTaskBoardData", () => ({
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

vi.mock("../modals/settings/CapabilitiesTab", () => ({ CapabilitiesTab: () => null }));

import { ContextModal } from "./index";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const emptyContext: GroupContext = {
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
    recent_decisions: [],
    recent_handoffs: [],
  },
  agent_states: [],
  tasks_summary: { total: 0, planned: 0, active: 0, done: 0, archived: 0 },
};

describe("ContextModal PROJECT.md loading and ownership", () => {
  let host: HTMLDivElement;
  let root: Root;
  const onOpenContext = vi.fn(async () => {});
  const onSyncContext = vi.fn(async () => {});

  const renderModal = async (groupId = "g_a", isOpen = true, context = emptyContext) => {
    await act(async () => {
      root.render(
        <ContextModal
          isOpen={isOpen}
          onClose={() => {}}
          groupId={groupId}
          groupTitle={groupId}
          context={context}
          onOpenContext={onOpenContext}
          onSyncContext={onSyncContext}
          isDark={false}
        />,
      );
    });
  };

  const clickButton = async (label: string) => {
    const btn = [...document.body.querySelectorAll<HTMLButtonElement>("button")].find((el) =>
      el.textContent?.includes(label),
    );
    expect(btn, `button "${label}"`).toBeDefined();
    await act(async () => {
      btn!.click();
    });
  };

  beforeEach(() => {
    vi.resetAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("reads the existing PROJECT.md using the Web endpoint's exists field", async () => {
    mocks.apiJson.mockResolvedValue({
      ok: true,
      result: { exists: true, path: "/a/PROJECT.md", content: "Existing repository charter" },
    });
    await renderModal();
    await clickButton("PROJECT.md");
    expect(host.textContent).toContain("Existing repository charter");
    expect(host.textContent).not.toContain("No PROJECT.md found");
  });

  it("initializes summary edits from the latest snapshot and retains an existing draft", async () => {
    const context: GroupContext = {
      ...emptyContext,
      coordination: {
        ...emptyContext.coordination,
        brief: {
          objective: "Current objective",
          current_focus: "Current focus",
          constraints: ["Keep existing behavior"],
          project_brief: "Current summary",
          project_brief_stale: true,
        },
      },
    };
    await renderModal();
    await renderModal("g_a", true, context);
    await clickButton("Summary needs refresh");
    expect([...host.querySelectorAll("input")].map((el) => el.value)).toContain(
      "Current objective",
    );
    const draft = [...host.querySelectorAll("textarea")].find(
      (el) => el.value === "Current summary",
    );
    expect(draft).toBeDefined();
    expect([...host.querySelectorAll("textarea")].map((el) => el.value)).toContain(
      "Keep existing behavior",
    );
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
      setter.call(draft, "Unsaved summary edit");
      draft!.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await clickButton("Summary needs refresh");
    expect(draft!.value).toBe("Unsaved summary edit");
  });

  it("requests PROJECT.md only once when the endpoint returns an error instead of looping infinitely", async () => {
    let calls = 0;
    mocks.apiJson.mockImplementation(async () => {
      calls += 1;
      if (calls > 5) throw new Error("Infinite request loop detected");
      return { ok: false, error: { code: "read_failed", message: "Disk read error" } };
    });

    await renderModal("g_a");
    await clickButton("PROJECT.md");

    expect(calls).toBe(1);
    expect(document.body.textContent).toContain("Disk read error");
  });

  it("does not leak Group A PROJECT.md content or edit state into Group B after switching groups", async () => {
    const readA = deferred<{ ok: true; result: ProjectMdInfo }>();
    const readB = deferred<{ ok: true; result: ProjectMdInfo }>();
    mocks.apiJson.mockImplementation((url: string) =>
      url.includes("g_a") ? readA.promise : readB.promise,
    );

    await renderModal("g_a");
    await clickButton("PROJECT.md");
    await clickButton("Create");

    // Switch to Group B while Group A's handleEditProject loadProjectMd(true) is in flight
    await renderModal("g_b");
    await clickButton("PROJECT.md");

    await act(async () => {
      readA.resolve({
        ok: true,
        result: { exists: true, path: "/a/PROJECT.md", content: "Secret Group A Constitution" },
      });
      await readA.promise;
    });

    expect(document.body.textContent).not.toContain("Secret Group A Constitution");
    expect(document.body.querySelector("textarea")).toBeNull();

    await act(async () => {
      readB.resolve({
        ok: true,
        result: { exists: true, path: "/b/PROJECT.md", content: "Group B Constitution" },
      });
      await readB.promise;
    });

    expect(document.body.textContent).toContain("Group B Constitution");
  });

  it("completes committed PROJECT.md writeback and team notification after modal closes", async () => {
    const saveReq = deferred<{ ok: true; result: ProjectMdInfo }>();
    mocks.apiJson.mockImplementation((url: string, init?: RequestInit) => {
      if (url.endsWith("/project_md") && init?.method === "PUT") {
        return saveReq.promise;
      }
      if (url.endsWith("/project_md")) {
        return Promise.resolve({
          ok: true,
          result: { exists: true, path: "/a/PROJECT.md", content: "Initial content" },
        });
      }
      if (url.endsWith("/send")) {
        return Promise.resolve({ ok: true, result: {} });
      }
      return Promise.resolve({ ok: true, result: {} });
    });

    await renderModal("g_a", true);
    await clickButton("PROJECT.md");
    await clickButton("Edit");

    const checkbox = document.body.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    await act(async () => {
      checkbox.click();
    });
    await clickButton("Save PROJECT.md");

    // Close the modal while PUT /project_md is in flight
    await renderModal("g_a", false);

    await act(async () => {
      saveReq.resolve({
        ok: true,
        result: { exists: true, path: "/a/PROJECT.md", content: "Updated content" },
      });
      await saveReq.promise;
    });

    expect(onSyncContext).toHaveBeenCalledTimes(1);
    expect(mocks.apiJson).toHaveBeenCalledWith(
      "/api/v1/groups/g_a/send",
      expect.objectContaining({ method: "POST" }),
    );
  });
});
