// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceSecretaryComposerControl } from "../VoiceSecretaryComposerControl";
import { useGroupStore, useModalStore, useUIStore } from "../../../stores";
import type { AssistantStateResult, SecretaryTaskSummary } from "../../../types";

const api = vi.hoisted(() => ({
  workspace: vi.fn(),
  status: vi.fn(),
  append: vi.fn(),
  clear: vi.fn(),
  cancel: vi.fn(),
  retry: vi.fn(),
  focus: vi.fn(),
  t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: api.t, i18n: { language: "en" } }),
}));
vi.mock("../../../services/api", async (original) => ({
  ...(await original<typeof import("../../../services/api")>()),
  fetchVoiceAssistantWorkspace: api.workspace,
  fetchVoiceAssistantStatus: api.status,
  appendVoiceAssistantInput: api.append,
  clearVoiceAssistantAskRequests: api.clear,
  cancelSecretaryTask: api.cancel,
  retrySecretaryTask: api.retry,
  fetchLatestVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
  fetchVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
  fetchSecretaryTasks: vi.fn(async (gid: string) => ({
    ok: true,
    result: { tasks: (await api.workspace(gid)).result.secretary_tasks, ready: true },
  })),
}));
vi.mock("./SecretaryRuntimePanel", () => ({ SecretaryRuntimePanel: () => null }));
vi.mock("../../../components/LazyMarkdownRenderer", () => ({
  LazyMarkdownRenderer: ({ content }: { content: string }) => <div>{content}</div>,
}));

function state(
  groupId: string,
  requestId = "",
  phase: SecretaryTaskSummary["phase"] = "queued",
  reply = "",
): AssistantStateResult {
  return {
    group_id: groupId,
    assistant: {
      assistant_id: "voice_secretary",
      kind: "voice_secretary",
      enabled: true,
      lifecycle: "ready",
      config: { recognition_backend: "browser_asr" },
      health: { secretary: { configured: true, ready: true } },
    },
    documents: [],
    ask_requests: requestId
      ? [
          {
            request_id: requestId,
            status: reply ? "done" : "pending",
            request_text: `Question ${groupId}`,
            reply_text: reply,
          },
        ]
      : [],
    secretary_tasks: requestId
      ? [
          {
            task_id: `task-${requestId}`,
            target: {
              group_id: groupId,
              scope_key: "scope",
              kind: "ask",
              document_path: "",
              request_id: requestId,
            },
            phase,
            cleanup_confirmed: phase === "done",
            created_at: "2026-10-10T00:00:00Z",
            updated_at: "2026-10-10T00:00:00Z",
            source_count: 1,
            preview: `Question ${groupId}`,
            diagnostic: "",
            projection_error: "",
            previous_task_id: "",
            superseded_by: "",
            candidate_available: false,
            projected_at: reply ? "2026-10-10T00:00:01Z" : "",
          },
        ]
      : [],
  };
}

describe("composer Ask request status", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let current: Record<string, AssistantStateResult>;
  let nextId: number;
  const render = (groupId = "A", initiallyOpen = true) =>
    act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId={groupId}
          isDark={false}
          busy=""
          variant="assistantRow"
          captureMode="instruction"
          initiallyOpen={initiallyOpen}
          onFocusComposer={api.focus}
        />,
      ),
    );
  const advance = (ms: number) => act(async () => vi.advanceTimersByTimeAsync(ms));
  const click = (selector: string) =>
    act(async () => document.querySelector<HTMLButtonElement>(selector)!.click());
  const summary = () => host.querySelector('[data-voice-composer-activity="ask"]');
  const submit = async () => {
    await act(async () => {
      const input = document.querySelector<HTMLTextAreaElement>("[data-voice-instruction-input]")!;
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        input,
        "Question A",
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => {
      const submitButton = Array.from(
        document.querySelectorAll<HTMLButtonElement>("[data-voice-input-bar] button"),
      ).find((button) => button.textContent === "voiceSecretaryAskDocumentButton")!;
      submitButton.click();
    });
    expect(api.append).toHaveBeenCalled();
  };
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.useFakeTimers();
    vi.clearAllMocks();
    window.sessionStorage.clear();
    nextId = 0;
    current = { A: state("A"), B: state("B") };
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    useUIStore.setState({ canAccessGlobalSettings: true, isSmallScreen: false });
    useGroupStore.setState({ chatByGroup: {} });
    useModalStore.setState((store) => ({ modals: { ...store.modals, settings: false } }));
    api.workspace.mockImplementation(async (gid: string) => ({
      ok: true,
      result: structuredClone(current[gid]),
    }));
    api.status.mockImplementation(async (gid: string) => ({
      ok: true,
      result: structuredClone(current[gid]),
    }));
    api.append.mockImplementation(async (gid: string) => {
      const id = `${gid}-${++nextId}`;
      current[gid] = state(gid, id);
      return { ok: true, result: { request_id: id } };
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("does not resurrect an unrelated historical pending Ask in the composer", async () => {
    current.A = state("A", "old-request");
    await render("A", false);
    expect(summary()).toBeNull();
  });

  it("observes the actual task stage after the workspace closes", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    current.A = state("A", "A-1", "running");
    await advance(2000);
    expect(api.status).toHaveBeenCalled();
    expect(summary()?.textContent).toContain("Working");
    expect(summary()?.textContent).not.toContain("Queued");
  });

  it("opens existing execution details from a pending summary", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    await click('[data-voice-composer-activity="ask"] button');
    expect(document.querySelector('[data-voice-body-mode="execution"]')).not.toBeNull();
  });

  it("hides only the summary and still receives the final reply without ledger events", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    await click("[data-voice-ask-hide]");
    expect(summary()).toBeNull();
    expect(api.clear).not.toHaveBeenCalled();
    expect(api.cancel).not.toHaveBeenCalled();
    expect(api.focus).toHaveBeenCalledTimes(1);
    current.A = state("A", "A-1", "done", "Final answer A");
    await advance(2000);
    expect(document.body.textContent).toContain("Final answer A");
    expect(host.querySelector('[aria-live="polite"]')?.textContent).toContain(
      "voiceSecretaryReplyReadyShort",
    );
  });

  it("retains hiding across Group changes and a remount but displays the next request", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    await click("[data-voice-ask-hide]");
    await render("B", false);
    await render("A", false);
    expect(summary()).toBeNull();
    await act(async () => root.unmount());
    root = createRoot(host);
    await render("A", false);
    expect(summary()).toBeNull();
    await act(async () =>
      host
        .querySelector<HTMLButtonElement>('button[aria-label="Expand Voice Secretary workspace"]')!
        .click(),
    );
    await submit();
    await click("[data-voice-sheet-close]");
    expect(summary()?.textContent).toContain("Queued");
    for (let index = 0; index < window.sessionStorage.length; index++) {
      const stored = window.sessionStorage.getItem(window.sessionStorage.key(index)!);
      expect(stored).not.toContain("Question A");
      expect(Object.keys(JSON.parse(stored!)).sort()).toEqual(["hidden", "requestId"]);
    }
  });

  it("does not overlap slow status reads or apply an old Group response", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    let finish!: (value: unknown) => void;
    api.status.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    api.status.mockClear();
    await advance(8000);
    expect(api.status).toHaveBeenCalledTimes(1);
    await render("B", false);
    await act(async () => finish({ ok: true, result: state("A", "A-1", "done", "Late answer A") }));
    expect(document.body.textContent).not.toContain("Late answer A");
    expect(summary()).toBeNull();
  });

  it("retains an observed terminal task outside the recent window until its result is projected", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    current.A = state("A", "A-1", "done");
    await advance(2000);
    expect(summary()?.textContent).not.toContain("Queued");
    current.A = { ...current.A, secretary_tasks: [] };
    await advance(4000);
    expect(summary()?.textContent).not.toContain("Queued");
    current.A = { ...state("A", "A-1", "done", "Projected answer A"), secretary_tasks: [] };
    current.A.ask_requests![0].secretary_task_id = "task-A-1";
    await advance(2000);
    expect(document.body.textContent).toContain("Projected answer A");
  });

  it("receives an authoritative result when the last observed running task leaves the window", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    current.A = state("A", "A-1", "running");
    await advance(2000);
    expect(summary()?.textContent).toContain("Working");
    current.A = { ...state("A", "A-1", "done", "Completed between reads"), secretary_tasks: [] };
    current.A.ask_requests![0].secretary_task_id = "task-A-1";
    await advance(2000);
    expect(document.body.textContent).toContain("Completed between reads");
    expect(summary()?.textContent).toContain("Reply ready");
    const settledCalls = api.status.mock.calls.length;
    await advance(4000);
    expect(api.status).toHaveBeenCalledTimes(settledCalls);
  });

  it.each([
    ["done", "Reply ready"],
    ["needs_user", "Needs input"],
    ["failed", "Failed"],
  ] as const)(
    "restores %s feedback without a task after switching Groups and remounting",
    async (status, label) => {
      await render();
      await submit();
      await click("[data-voice-sheet-close]");
      current.A = { ...state("A", "A-1", "done", "Restored result"), secretary_tasks: [] };
      current.A.ask_requests![0].status = status;
      current.A.ask_requests![0].secretary_task_id = "task-A-1";
      await render("B", false);
      await render("A", false);
      expect(summary()?.textContent).toContain(label);
      expect(summary()?.textContent).not.toContain("Queued");
      await act(async () => root.unmount());
      root = createRoot(host);
      await render("A", false);
      expect(summary()?.textContent).toContain(label);
      expect(summary()?.textContent).not.toContain("Queued");
      const settledCalls = api.status.mock.calls.length;
      await advance(4000);
      expect(api.status).toHaveBeenCalledTimes(settledCalls);
    },
  );

  it.each(["settled", "inflight", "historical"])(
    "restarts observation from a real execution-view retry (%s) without its old reply",
    async (scenario) => {
      await render();
      await submit();
      await click("[data-voice-sheet-close]");
      current.A = state("A", "A-1", "needs_user", "Old clarification");
      current.A.ask_requests![0].status = "needs_user";
      current.A.secretary_tasks![0].cleanup_confirmed = true;
      let finishOldRead: ((value: unknown) => void) | undefined;
      const oldSnapshot = structuredClone(current.A);
      if (scenario === "inflight")
        api.status.mockImplementationOnce(
          () =>
            new Promise((resolve) => {
              finishOldRead = resolve;
            }),
        );
      await advance(2000);
      const settledCalls = api.status.mock.calls.length;
      await advance(4000);
      expect(api.status).toHaveBeenCalledTimes(settledCalls);
      await click("[data-voice-ask-hide]");
      const retriedRequestId = scenario === "historical" ? "history-A" : "A-1";
      if (scenario === "historical") {
        const history = state("A", retriedRequestId, "needs_user", "Earlier clarification");
        history.secretary_tasks![0].cleanup_confirmed = true;
        history.secretary_tasks![0].preview = "Earlier question A";
        history.ask_requests![0].status = "needs_user";
        current.A.secretary_tasks!.push(history.secretary_tasks![0]);
        current.A.ask_requests!.push(history.ask_requests![0]);
      }
      await act(async () =>
        host
          .querySelector<HTMLButtonElement>(
            'button[aria-label="Expand Voice Secretary workspace"]',
          )!
          .click(),
      );
      await click("[data-voice-workstage-toggle]");
      const history = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(
        (button) => button.textContent?.includes("voiceSettings.recentTasks"),
      )!;
      await act(async () => history.click());
      const subject = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(
        (button) =>
          button.textContent?.includes(
            scenario === "historical" ? "Earlier question A" : "Question A",
          ) && button.textContent?.includes("voiceSettings.phases.needs_user"),
      );
      expect(subject).not.toBeUndefined();
      await act(async () => subject!.click());
      const input = document.querySelector<HTMLTextAreaElement>(
        'textarea[aria-label="voiceSettings.followup"]',
      )!;
      expect(input).not.toBeNull();
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
          input,
          "Clarification",
        );
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
      api.retry.mockImplementationOnce(async () => {
        const next = state("A", retriedRequestId).secretary_tasks![0];
        next.task_id = "task-retry";
        next.previous_task_id = `task-${retriedRequestId}`;
        next.created_at = "2026-10-10T00:01:00Z";
        current.A = { ...current.A, secretary_tasks: [next] };
        if (finishOldRead)
          api.workspace.mockImplementationOnce(async () => {
            // The control has been accepted; finish the prior read before React's effect cleanup.
            finishOldRead!({ ok: true, result: oldSnapshot });
            return { ok: true, result: structuredClone(current.A) };
          });
        return { ok: true, result: { task: structuredClone(next) } };
      });
      const retry = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(
        (button) => button.textContent === "voiceSettings.continueTask",
      )!;
      expect(retry.disabled).toBe(false);
      await act(async () => retry.click());
      expect(api.retry).toHaveBeenCalledWith(
        "A",
        `task-${retriedRequestId}`,
        "Clarification",
        false,
      );
      await click("[data-voice-sheet-close]");
      expect(summary()?.textContent).toContain("Queued");
      expect(summary()?.textContent).toContain("Question A");
      expect(summary()?.textContent).not.toContain("Old clarification");
      expect(summary()?.textContent).not.toContain("Reply ready");
      expect(document.querySelector('[aria-label="Voice Secretary reply"]')).toBeNull();
      await advance(2000);
      expect(api.status.mock.calls.length).toBeGreaterThan(settledCalls);
      current.A.secretary_tasks![0].phase = "done";
      current.A.secretary_tasks![0].cleanup_confirmed = true;
      // Even a late timestamp on the old Ask reply cannot settle the new task.
      current.A.ask_requests![0].updated_at = "2026-10-10T00:02:00Z";
      await advance(2000);
      expect(summary()?.textContent).toContain("Receiving result");
      const feedbackIndex = current.A.ask_requests!.findIndex(
        (item) => item.request_id === retriedRequestId,
      );
      current.A.ask_requests!.splice(feedbackIndex, 1);
      current.A.ask_requests!.unshift({
        request_id: retriedRequestId,
        status: "done",
        request_text: "Question A",
        reply_text: "Retry answer A",
        secretary_task_id: "task-retry",
      });
      current.A.secretary_tasks = [];
      await advance(2000);
      expect(document.body.textContent).toContain("Retry answer A");
    },
  );

  it("preserves a current Ask result against an older workspace response", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    let finish!: (value: unknown) => void;
    api.workspace.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await act(async () => window.dispatchEvent(new Event("focus")));
    current.A = state("A", "A-1", "running");
    await advance(2000);
    expect(summary()?.textContent).toContain("Working");
    await act(async () => finish({ ok: true, result: state("A", "A-1", "queued") }));
    expect(summary()?.textContent).toContain("Working");
  });

  it("shows a saved request rather than claiming it is queued when no execution owner is ready", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    current.A.secretary_tasks = [];
    current.A.assistant!.health = {
      secretary: { configured: true, ready: false, readiness_code: "owner_unavailable" },
    };
    // Begin a distinct accepted request whose admission has been deferred.
    await act(async () =>
      host
        .querySelector<HTMLButtonElement>('button[aria-label="Expand Voice Secretary workspace"]')!
        .click(),
    );
    api.append.mockImplementationOnce(async () => {
      current.A = {
        ...current.A,
        ask_requests: [{ request_id: "A-2", status: "pending", request_text: "Question A" }],
      };
      return { ok: true, result: { request_id: "A-2", secretary_processing_deferred: true } };
    });
    await submit();
    await click("[data-voice-sheet-close]");
    expect(summary()?.textContent).toContain("Request saved");
    expect(summary()?.textContent).not.toContain("Queued");
  });

  it("keeps the last task snapshot on a read failure and recovers through reads only", async () => {
    await render();
    await submit();
    await click("[data-voice-sheet-close]");
    current.A = state("A", "A-1", "running");
    await advance(2000);
    api.status.mockRejectedValueOnce(new Error("Status connection interrupted"));
    await advance(2000);
    expect(summary()?.textContent).toContain("Status not updated");
    expect(summary()?.textContent).not.toContain("Failed");
    await click('[data-voice-composer-activity="ask"] button');
    expect(document.querySelector("[data-secretary-stage]")?.textContent).toContain(
      "voiceSettings.phases.running",
    );
    await click("[data-voice-sheet-close]");
    await advance(2000);
    expect(summary()?.textContent).toContain("Working");
    expect(api.append).toHaveBeenCalledTimes(1);
    expect(api.cancel).not.toHaveBeenCalled();
  });

  it("retains a late accepted request in its original Group without displaying it in the new Group", async () => {
    await render();
    let finish!: (value: unknown) => void;
    api.append.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await submit();
    await render("B", false);
    current.A = state("A", "accepted-A", "running");
    await act(async () => finish({ ok: true, result: { request_id: "accepted-A" } }));
    expect(summary()).toBeNull();
    await render("A", false);
    expect(summary()?.textContent).toContain("Working");
    expect(summary()?.textContent).toContain("Question A");
    expect(api.append).toHaveBeenCalledTimes(1);
  });
});
