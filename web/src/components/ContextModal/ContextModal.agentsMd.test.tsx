// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupContext, AgentsMdInfo } from "../../types";

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
    t: (_key: string, opts?: { defaultValue?: string; [key: string]: unknown }) =>
      (opts?.defaultValue || _key).replace(/\{\{(\w+)\}\}/g, (match, key: string) =>
        opts?.[key] === undefined ? match : String(opts[key]),
      ),
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

vi.mock("../modals/settings/CapabilitiesTab", () => ({
  CapabilitiesTab: () => <div>Group skill candidates</div>,
}));

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

describe("ContextModal AGENTS.md loading and ownership", () => {
  let host: HTMLDivElement;
  let root: Root;
  const onClose = vi.fn();
  const onOpenContext = vi.fn(async () => {});
  const onSyncContext = vi.fn(async () => {});

  const renderModal = async (groupId = "g_a", isOpen = true, context = emptyContext) => {
    await act(async () => {
      root.render(
        <ContextModal
          isOpen={isOpen}
          onClose={onClose}
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

  const clickTab = async (label: string) => {
    const tab = [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find(
      (el) => el.textContent === label,
    );
    expect(tab, `tab ${label}`).toBeDefined();
    await act(async () => tab!.click());
    return tab!;
  };
  const changeText = async (element: HTMLInputElement | HTMLTextAreaElement, text: string) => {
    await act(async () => {
      const proto =
        element instanceof HTMLInputElement
          ? HTMLInputElement.prototype
          : HTMLTextAreaElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(element, text);
      element.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };

  it("gives each section its own content area and keeps task filters on return", async () => {
    mocks.apiJson.mockResolvedValue({
      ok: true,
      result: { exists: true, path: "/a/AGENTS.md", content: "Repository rules" },
    });
    await renderModal();
    const tabs = [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
    expect(host.querySelectorAll('[role="tablist"]')).toHaveLength(1);
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "Tasks",
      "AGENTS.md",
      "Agent State",
      "Coordination log",
      "Skills",
    ]);
    for (const tab of tabs) {
      expect(
        document
          .getElementById(tab.getAttribute("aria-controls")!)
          ?.getAttribute("aria-labelledby"),
      ).toBe(tab.id);
    }
    await changeText(
      host.querySelector<HTMLInputElement>(
        'input[aria-label="Search tasks by title, id, assignee, or outcome"]',
      )!,
      "Keep this task filter",
    );
    for (const [tab, content] of [
      ["AGENTS.md", "Repository rules"],
      ["Coordination log", "Recent decisions"],
      ["Agent State", "No agent state"],
      ["Skills", "Group skill candidates"],
    ]) {
      await clickTab(tab);
      expect(host.querySelectorAll('[role="tabpanel"]:not([hidden])')).toHaveLength(1);
      expect(host.textContent).toContain(content);
      expect(
        host.querySelector('input[aria-label="Search tasks by title, id, assignee, or outcome"]'),
      ).toBeNull();
      expect(host.textContent).not.toContain("New task");
    }
    await clickTab("Tasks");
    expect(
      host.querySelector<HTMLInputElement>(
        'input[aria-label="Search tasks by title, id, assignee, or outcome"]',
      )?.value,
    ).toBe("Keep this task filter");
    expect(mocks.updateCoordinationTask).not.toHaveBeenCalled();
    expect(mocks.updateCoordinationBrief).not.toHaveBeenCalled();
    expect(mocks.addCoordinationNote).not.toHaveBeenCalled();
  });

  it("moves focus with arrows without activating or fetching another section", async () => {
    await renderModal();
    const tabs = [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
    tabs[0].focus();
    await act(async () =>
      tabs[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true })),
    );
    expect(document.activeElement).toBe(tabs[1]);
    expect(tabs[0].getAttribute("aria-selected")).toBe("true");
    expect(mocks.apiJson).not.toHaveBeenCalled();
    await act(async () =>
      tabs[1].dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true })),
    );
    expect(document.activeElement).toBe(tabs[4]);
    await act(async () =>
      tabs[4].dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })),
    );
    expect(document.activeElement).toBe(tabs[0]);
  });

  it("retains file and summary drafts across sections and still guards closing", async () => {
    mocks.apiJson.mockResolvedValue({
      ok: true,
      result: { exists: true, path: "/a/AGENTS.md", content: "Saved rules" },
    });
    await renderModal();
    await clickButton("Edit");
    await changeText(
      host.querySelector<HTMLTextAreaElement>("textarea")!,
      "Unsaved summary constraint",
    );
    await clickTab("AGENTS.md");
    await clickButton("Edit");
    await changeText(
      host.querySelector<HTMLTextAreaElement>('textarea[aria-label="AGENTS.md"]')!,
      "Unsaved repository rules",
    );
    await clickTab("Tasks");
    expect([...host.querySelectorAll("textarea")].map((el) => el.value)).toContain(
      "Unsaved summary constraint",
    );
    expect(host.querySelector('button[aria-expanded="true"]')).not.toBeNull();
    await clickTab("AGENTS.md");
    expect(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="AGENTS.md"]')?.value).toBe(
      "Unsaved repository rules",
    );
    const confirm = vi.fn(() => false);
    vi.stubGlobal("confirm", confirm);
    try {
      await act(async () =>
        host.querySelector<HTMLButtonElement>('button[aria-label="Close context modal"]')!.click(),
      );
      expect(confirm).toHaveBeenCalledOnce();
      expect(onClose).not.toHaveBeenCalled();
      expect(mocks.apiJson.mock.calls.filter(([, init]) => init?.method === "PUT")).toHaveLength(0);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("keeps note drafts across sections and submits only on request", async () => {
    mocks.addCoordinationNote.mockResolvedValue({ ok: true, result: {} });
    await renderModal();
    await clickTab("Coordination log");
    await changeText(host.querySelector<HTMLTextAreaElement>("textarea")!, "Keep this decision");
    await clickTab("Tasks");
    expect(mocks.addCoordinationNote).not.toHaveBeenCalled();
    await clickTab("Coordination log");
    expect(host.querySelector("textarea")?.value).toBe("Keep this decision");
    await clickButton("Add decision");
    expect(mocks.addCoordinationNote).toHaveBeenCalledOnce();
    expect(mocks.addCoordinationNote.mock.calls[0]).toEqual([
      "g_a",
      "decision",
      "Keep this decision",
      null,
    ]);
  });

  it("reads the existing AGENTS.md using the Web endpoint's exists field", async () => {
    mocks.apiJson.mockResolvedValue({
      ok: true,
      result: { exists: true, path: "/a/AGENTS.md", content: "Existing repository instructions" },
    });
    await renderModal();
    await clickButton("AGENTS.md");
    expect(host.textContent).toContain("Existing repository instructions");
    expect(host.textContent).toContain("/a/AGENTS.md");
    expect(host.textContent).toContain("saving does not reload running sessions");
    expect(host.textContent).not.toContain("No AGENTS.md in this workspace.");
  });

  it.each([false, true])(
    "edits a missing or empty AGENTS.md without a generated template (exists=%s)",
    async (exists) => {
      mocks.apiJson.mockResolvedValue({
        ok: true,
        result: { exists, path: "/a/AGENTS.md", content: "" },
      });
      await renderModal();
      await clickButton("AGENTS.md");
      expect(host.textContent).toContain(
        exists ? "AGENTS.md is empty." : "No AGENTS.md in this workspace.",
      );
      await clickButton(exists ? "Edit" : "Create");
      const editor = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="AGENTS.md"]')!;
      expect(editor.value).toBe("");
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
          editor,
          "# Repository instructions",
        );
        editor.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await clickButton("Save AGENTS.md");
      const put = mocks.apiJson.mock.calls.find(([, init]) => init?.method === "PUT");
      expect(put?.[0]).toBe("/api/v1/groups/g_a/agents_md");
      expect(JSON.parse(put?.[1].body)).toEqual({
        content: "# Repository instructions",
        expected_path: "/a/AGENTS.md",
      });
      expect(mocks.apiJson.mock.calls.some(([url]) => url.endsWith("/send"))).toBe(false);
      expect(onSyncContext).not.toHaveBeenCalled();
    },
  );

  it("retains the instructions draft and skips notification when the workspace changed", async () => {
    mocks.apiJson.mockImplementation(async (_url: string, init?: RequestInit) =>
      init?.method === "PUT"
        ? {
            ok: false,
            error: {
              code: "workspace_changed",
              message: "Workspace changed. Reopen AGENTS.md before saving.",
            },
          }
        : { ok: true, result: { exists: true, path: "/a/AGENTS.md", content: "Keep this draft" } },
    );
    await renderModal();
    await clickButton("AGENTS.md");
    await clickButton("Edit");
    await act(async () => host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    await clickButton("Save AGENTS.md");
    expect(host.querySelector<HTMLTextAreaElement>('textarea[aria-label="AGENTS.md"]')?.value).toBe(
      "Keep this draft",
    );
    expect(host.textContent).toContain("Workspace changed.");
    expect(mocks.apiJson.mock.calls.some(([url]) => url.endsWith("/send"))).toBe(false);
    expect(onSyncContext).not.toHaveBeenCalled();
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

  it("requests AGENTS.md only once when the endpoint returns an error instead of looping infinitely", async () => {
    let calls = 0;
    mocks.apiJson.mockImplementation(async () => {
      calls += 1;
      if (calls > 5) throw new Error("Infinite request loop detected");
      return { ok: false, error: { code: "read_failed", message: "Disk read error" } };
    });

    await renderModal("g_a");
    await clickButton("AGENTS.md");

    expect(calls).toBe(1);
    expect(document.body.textContent).toContain("Disk read error");
    await clickTab("Tasks");
    await clickTab("AGENTS.md");
    expect(calls).toBe(2);
  });

  it("does not leak Group A AGENTS.md content or edit state into Group B after switching groups", async () => {
    const readA = deferred<{ ok: true; result: AgentsMdInfo }>();
    const readB = deferred<{ ok: true; result: AgentsMdInfo }>();
    mocks.apiJson.mockImplementation((url: string) =>
      url.includes("g_a") ? readA.promise : readB.promise,
    );

    await renderModal("g_a");
    await clickButton("AGENTS.md");
    await clickButton("Create");

    // Switch to Group B while Group A's handleEditAgentsMd loadAgentsMd(true) is in flight
    await renderModal("g_b");
    await clickButton("AGENTS.md");

    await act(async () => {
      readA.resolve({
        ok: true,
        result: { exists: true, path: "/a/AGENTS.md", content: "Secret Group A Constitution" },
      });
      await readA.promise;
    });

    expect(document.body.textContent).not.toContain("Secret Group A Constitution");
    expect(document.body.querySelector("textarea")).toBeNull();

    await act(async () => {
      readB.resolve({
        ok: true,
        result: { exists: true, path: "/b/AGENTS.md", content: "Group B Constitution" },
      });
      await readB.promise;
    });

    expect(document.body.textContent).toContain("Group B Constitution");
  });

  it("completes a requested team notification after closing without writing Context", async () => {
    const saveReq = deferred<{ ok: true; result: AgentsMdInfo }>();
    mocks.apiJson.mockImplementation((url: string, init?: RequestInit) => {
      if (url.endsWith("/agents_md") && init?.method === "PUT") {
        return saveReq.promise;
      }
      if (url.endsWith("/agents_md")) {
        return Promise.resolve({
          ok: true,
          result: { exists: true, path: "/a/AGENTS.md", content: "Initial content" },
        });
      }
      if (url.endsWith("/send")) {
        return Promise.resolve({ ok: true, result: {} });
      }
      return Promise.resolve({ ok: true, result: {} });
    });

    await renderModal("g_a", true);
    await clickButton("AGENTS.md");
    await clickButton("Edit");

    const checkbox = document.body.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    await act(async () => {
      checkbox.click();
    });
    await clickButton("Save AGENTS.md");

    // Close the modal while PUT /agents_md is in flight
    await renderModal("g_a", false);

    await act(async () => {
      saveReq.resolve({
        ok: true,
        result: { exists: true, path: "/a/AGENTS.md", content: "Updated content" },
      });
      await saveReq.promise;
    });

    expect(onSyncContext).not.toHaveBeenCalled();
    expect(mocks.apiJson).toHaveBeenCalledWith(
      "/api/v1/groups/g_a/send",
      expect.objectContaining({ method: "POST" }),
    );
    const sent = mocks.apiJson.mock.calls.find(([url]) => url.endsWith("/send"))!;
    const sentBody = JSON.parse(sent[1].body);
    expect(sentBody.message_mode).toBe("send");
    expect(sentBody.to).toEqual(["@all"]);
    expect(sentBody.text).toBe(
      "Repository instructions updated at /a/AGENTS.md. Re-read this file if it applies to your work.",
    );
  });
});
