// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { SecretaryTaskSummary } from "../../../types";
const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  cancel: vi.fn(),
  retry: vi.fn(),
  candidate: vi.fn(),
  forward: vi.fn(),
  terminal: vi.fn(),
  runtime: vi.fn(),
  t: (key: string, values?: { scope?: string }) =>
    values?.scope ? `${key}: ${values.scope}` : key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
vi.mock("../../../services/api", () => ({
  fetchSecretaryTasks: mocks.fetch,
  fetchSecretaryRuntime: mocks.runtime,
  cancelSecretaryTask: mocks.cancel,
  retrySecretaryTask: mocks.retry,
  fetchSecretaryCandidate: mocks.candidate,
  forwardSecretaryProposal: mocks.forward,
  getSecretaryTerminalWebSocketUrl: () => "ws://fixture/terminal",
}));
vi.mock("../../../features/voice/NativeSessionTerminal", () => ({
  NativeSessionTerminal: (props: { generation: string; readOnly?: boolean }) => {
    mocks.terminal(props);
    return <div data-native-fixture={props.generation} />;
  },
}));
vi.mock("../../../components/LazyMarkdownRenderer", () => ({
  LazyMarkdownRenderer: ({ content }: { content: string }) => <p>{content}</p>,
}));
import { secretaryTaskOutcome } from "./secretaryTaskPresentation";
import { SecretaryTasks } from "./SecretaryTasks";
import { useUIStore } from "../../../stores/useUIStore";
const task: SecretaryTaskSummary = {
  task_id: "a-task",
  target: {
    group_id: "A",
    scope_key: "scope-a",
    kind: "ask",
    document_path: "",
    request_id: "request-a",
  },
  phase: "queued",
  cleanup_confirmed: true,
  created_at: "2026-10-10T01:00:00Z",
  updated_at: "2026-10-10T01:00:00Z",
  source_count: 1,
  preview: "A input",
  diagnostic: "",
  projection_error: "",
  previous_task_id: "",
  superseded_by: "",
  candidate_available: false,
};
const response = (tasks: SecretaryTaskSummary[]) => ({
  ok: true,
  result: { tasks, configured: true, ready: true, readiness_code: null, readiness_error: null },
});
describe("Group-owned secretary task controls", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    useUIStore.setState({ canAccessGlobalSettings: true });
    for (const mock of [
      mocks.fetch,
      mocks.cancel,
      mocks.retry,
      mocks.candidate,
      mocks.forward,
      mocks.terminal,
    ])
      mock.mockReset();
    mocks.fetch.mockResolvedValue(response([task]));
    mocks.runtime.mockResolvedValue({ ok: true, result: { phase: "not_started" } });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  });
  const render = (groupId = "A", active = true) =>
    act(async () =>
      root.render(<SecretaryTasks groupId={groupId} active={active} isDark={false} />),
    );
  const button = (label: string) =>
    Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (b) => b.textContent === label,
    )!;
  it("shows unavailable execution even when no task could be admitted", async () => {
    const diagnostic =
      "Secretary startup failed while reading Actor identities in Group g_000000000001: invalid YAML data at line 2 column 1; saved inputs are retained";
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [],
        configured: true,
        ready: false,
        readiness_code: "owner_unavailable",
        readiness_error: diagnostic,
      },
    });
    await render();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(diagnostic);
    expect(host.querySelector("[data-secretary-tasks]")).not.toBeNull();
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [],
        configured: true,
        ready: true,
        readiness_code: null,
        readiness_error: null,
      },
    });
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector("[data-secretary-tasks]")).toBeNull();
    expect(mocks.retry).not.toHaveBeenCalled();
  });
  it("shows one owner diagnostic in the runtime overview and preserves independent task reads", async () => {
    const diagnostic = "Synthetic owner startup failure";
    const readiness = {
      configured: true,
      ready: false,
      readiness_code: "owner_unavailable",
      readiness_error: diagnostic,
    };
    mocks.fetch.mockResolvedValue({ ok: true, result: { ...readiness, tasks: [] } });
    mocks.runtime.mockResolvedValue({
      ok: true,
      result: { ...readiness, phase: "unavailable", runtime: "codex", diagnostic },
    });
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} workspace />),
    );
    expect(
      [...host.querySelectorAll('[role="alert"]')].filter(
        (node) => node.textContent === diagnostic,
      ),
    ).toHaveLength(1);
    expect(host.querySelector("[data-secretary-runtime]")?.textContent).toContain(diagnostic);
    mocks.fetch.mockResolvedValue({
      ok: false,
      error: { message: "Independent task-list read failure" },
    });
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.textContent).toContain("Independent task-list read failure");
    expect(host.querySelector("[data-secretary-runtime]")?.textContent).toContain(diagnostic);
  });
  it("does not hide a later failed read when the workspace banner owns readiness feedback", async () => {
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [],
        configured: true,
        ready: false,
        readiness_code: "owner_unavailable",
        readiness_error: "Known readiness failure",
      },
    });
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} hideReadinessError />),
    );
    expect(host.textContent).not.toContain("Known readiness failure");
    mocks.fetch.mockResolvedValue({ ok: false, error: { message: "Later network read failure" } });
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Later network read failure");
  });
  it("keeps a failed task-list read even when its message equals the known readiness reason", async () => {
    const diagnostic = "Same transport and readiness message";
    const readiness = {
      configured: true,
      ready: false,
      readiness_code: "owner_unavailable",
      readiness_error: diagnostic,
    };
    mocks.fetch.mockResolvedValue({ ok: true, result: { ...readiness, tasks: [] } });
    mocks.runtime.mockResolvedValue({
      ok: true,
      result: { ...readiness, phase: "unavailable", diagnostic },
    });
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} workspace />),
    );
    mocks.fetch.mockResolvedValue({ ok: false, error: { message: diagnostic } });
    await act(async () => vi.advanceTimersByTime(2000));
    const taskReadAlert = host.querySelector('[data-secretary-tasks] > div [role="alert"]');
    expect(taskReadAlert?.closest("[data-secretary-runtime]")).toBeNull();
    expect(taskReadAlert?.textContent).toBe(diagnostic);
  });
  it("keeps public readiness feedback when the runtime view is restricted", async () => {
    useUIStore.setState({ canAccessGlobalSettings: false });
    const diagnostic = "Public owner-unavailable reason";
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [],
        configured: true,
        ready: false,
        readiness_code: "owner_unavailable",
        readiness_error: diagnostic,
      },
    });
    const runtimeReads = mocks.runtime.mock.calls.length;
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} workspace />),
    );
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(diagnostic);
    expect(host.textContent).toContain("voiceSettings.resident.adminOnly");
    expect(mocks.runtime).toHaveBeenCalledTimes(runtimeReads);
  });
  it("preserves the runtime view's independent GET failure", async () => {
    const readiness = {
      configured: true,
      ready: false,
      readiness_code: "owner_unavailable",
      readiness_error: "Known owner startup reason",
    };
    mocks.fetch.mockResolvedValue({ ok: true, result: { ...readiness, tasks: [] } });
    mocks.runtime.mockResolvedValue({ ok: true, result: { ...readiness, phase: "unavailable" } });
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} workspace />),
    );
    mocks.runtime.mockResolvedValue({
      ok: false,
      error: { message: "Independent runtime GET failure" },
    });
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.querySelector('[data-secretary-runtime] [role="alert"]')?.textContent).toBe(
      "Independent runtime GET failure",
    );
  });
  it("retains readiness on a selected task and always exposes failed task actions", async () => {
    const diagnostic = "Owner startup failure with accepted task";
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [task],
        configured: true,
        ready: false,
        readiness_code: "owner_unavailable",
        readiness_error: diagnostic,
      },
    });
    await act(async () =>
      root.render(
        <SecretaryTasks groupId="A" active isDark={false} workspace initialTaskId="a-task" />,
      ),
    );
    expect(host.textContent).toContain(diagnostic);
    expect(host.querySelector("[data-secretary-runtime]")).toBeNull();
    mocks.cancel.mockResolvedValue({ ok: false, error: { message: "Task cancellation failed" } });
    await act(async () => button("voiceSettings.cancelTask").click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Task cancellation failed");
  });
  it("opens the native task view only in the explicit workspace and drops it on Group navigation", async () => {
    const running = {
      ...task,
      phase: "running" as const,
      cleanup_confirmed: false,
      execution: {
        generation: "A-generation",
        runtime: "grok",
        native_terminal: true,
        progress: "",
      },
    };
    mocks.fetch.mockResolvedValue(response([running]));
    await render();
    expect(mocks.terminal).not.toHaveBeenCalled();
    await act(async () =>
      root.render(
        <SecretaryTasks groupId="A" active isDark={false} workspace initialTaskId="a-task" />,
      ),
    );
    expect(host.querySelector('[data-native-fixture="A-generation"]')).not.toBeNull();
    expect(mocks.terminal.mock.lastCall?.[0].readOnly).toBeFalsy();
    expect(mocks.terminal.mock.lastCall?.[0].scopeId).toBe("voice-secretary");
    mocks.fetch.mockResolvedValue(response([]));
    await act(async () =>
      root.render(<SecretaryTasks groupId="B" active isDark={false} workspace />),
    );
    expect(host.querySelector("[data-native-fixture]")).toBeNull();
    expect(host.textContent).not.toContain("A input");
    expect(mocks.cancel).not.toHaveBeenCalled();
  });
  it("shows ACP progress and keeps cancellation bound to the viewed task", async () => {
    mocks.fetch.mockResolvedValue(
      response([
        {
          ...task,
          phase: "running",
          cleanup_confirmed: false,
          execution: {
            generation: "ACP-A",
            runtime: "copilot",
            native_terminal: false,
            progress: "Checking the requested file.",
          },
        },
      ]),
    );
    await act(async () =>
      root.render(
        <SecretaryTasks groupId="A" active isDark={false} workspace initialTaskId="a-task" />,
      ),
    );
    expect(host.querySelector("[data-native-fixture]")).toBeNull();
    expect(host.textContent).toContain("Checking the requested file.");
    expect(host.textContent).toContain("scope-a");
    expect(host.querySelector("[data-secretary-tasks] ul")).toBeNull();
    expect(host.querySelector("[data-secretary-stage] details")?.hasAttribute("open")).toBe(false);
    mocks.cancel.mockResolvedValue({ ok: true, result: { cancel_requested: true } });
    mocks.fetch.mockResolvedValue(response([{ ...task, phase: "cancelled" }]));
    await act(async () => button("voiceSettings.cancelTask").click());
    expect(mocks.cancel).toHaveBeenCalledWith("A", "a-task");
    expect(host.textContent).toContain("voiceSettings.phases.cancelled");
    expect(mocks.retry).not.toHaveBeenCalled();
  });
  it("offers a selected Prompt follow-up without requiring task history to be opened", async () => {
    mocks.fetch.mockResolvedValue(
      response([
        {
          ...task,
          target: { ...task.target, kind: "prompt" },
          phase: "needs_user",
          receipt: { status: "needs_user", output: { reply_text: "Which audience?" } },
        },
      ]),
    );
    await act(async () =>
      root.render(
        <SecretaryTasks groupId="A" active isDark={false} workspace initialTaskId="a-task" />,
      ),
    );
    const field = host.querySelector<HTMLTextAreaElement>("textarea")!;
    expect(field).not.toBeNull();
    expect(field.closest("[hidden]")).toBeNull();
    expect(host.querySelectorAll("textarea")).toHaveLength(1);
    expect(host.textContent).toContain("Which audience?");
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        field,
        "New users",
      );
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    mocks.retry.mockResolvedValue({ ok: true, result: {} });
    await act(async () => button("voiceSettings.continueTask").click());
    expect(mocks.retry).toHaveBeenCalledWith("A", "a-task", "New users", false);
  });
  it("shows the settled result over residual progress and only claims application after acknowledgment", async () => {
    const done: SecretaryTaskSummary = {
      ...task,
      target: { ...task.target, kind: "prompt" },
      phase: "done",
      receipt: { status: "done", output: { draft_text: "Final draft" } },
      execution: {
        generation: "old",
        runtime: "copilot",
        native_terminal: false,
        progress: "Partial output",
      },
    };
    const navigate = vi.fn();
    mocks.fetch.mockResolvedValue(response([done]));
    await act(async () =>
      root.render(
        <SecretaryTasks
          groupId="A"
          active
          isDark={false}
          workspace
          initialTaskId="a-task"
          onOpenTarget={navigate}
        />,
      ),
    );
    expect(host.textContent).toContain("Final draft");
    expect(host.textContent).not.toContain("Partial output");
    expect(host.textContent).toContain("voiceSettings.outcome.draftReady");
    expect(host.textContent).not.toContain("voiceSettings.outcome.applied");
    await act(async () => button("voiceSettings.openTarget.prompt").click());
    expect(navigate).toHaveBeenCalledWith(done);
    expect(mocks.retry).not.toHaveBeenCalled();
    mocks.fetch.mockResolvedValue(response([{ ...done, prompt_draft_status: "applied" }]));
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.textContent).toContain("voiceSettings.outcome.applied");
  });
  it("uses one history control and preserves a chosen completed task when newer work appears", async () => {
    const older: SecretaryTaskSummary = {
      ...task,
      task_id: "older",
      phase: "done",
      preview: "Earlier question",
      created_at: "2026-10-09T01:00:00Z",
    };
    mocks.fetch.mockResolvedValue(response([task, older]));
    await act(async () =>
      root.render(
        <SecretaryTasks groupId="A" active isDark={false} workspace initialTaskId="a-task" />,
      ),
    );
    await act(async () => button("voiceSettings.recentTasks").click());
    const choices = host.querySelectorAll<HTMLButtonElement>("ul button");
    expect(choices).toHaveLength(2);
    expect(choices[1].textContent).toContain("Earlier question");
    expect(choices[1].querySelector("time")?.dateTime).toBe(older.created_at);
    await act(async () => choices[1].click());
    expect(host.querySelector("ul")).toBeNull();
    expect(document.activeElement).toBe(button("voiceSettings.recentTasks"));
    mocks.fetch.mockResolvedValue(response([{ ...task, task_id: "new", phase: "running" }, older]));
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.querySelector("[data-secretary-stage] h4")?.textContent).toBe("Earlier question");
    expect(host.querySelector('[role="combobox"]')).toBeNull();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });
  it("orders recent history by creation time without prioritizing unresolved or recently updated tasks", async () => {
    const waiting: SecretaryTaskSummary = {
      ...task,
      task_id: "older-waiting",
      phase: "needs_user",
      preview: "Earlier question needs details",
      created_at: "2026-10-09T01:00:00Z",
      updated_at: "2026-10-10T04:00:00Z",
      receipt: { status: "needs_user", output: { reply_text: "Which date?" } },
    };
    const latest: SecretaryTaskSummary = {
      ...task,
      task_id: "latest-done",
      phase: "done",
      preview: "Latest completed question",
      created_at: "2026-10-10T03:00:00Z",
    };
    const middle: SecretaryTaskSummary = {
      ...latest,
      task_id: "middle-done",
      preview: "Previous completed question",
      created_at: "2026-10-10T02:00:00Z",
    };
    mocks.fetch.mockResolvedValue(response([waiting, latest, middle]));
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} workspace />),
    );
    await act(async () => button("voiceSettings.recentTasks").click());
    const times = () =>
      Array.from(host.querySelectorAll<HTMLTimeElement>("ul time"), (time) => time.dateTime);
    expect(times()).toEqual([latest.created_at, middle.created_at, waiting.created_at]);
    expect(host.querySelector("ul li:last-child")?.textContent).toContain(
      "voiceSettings.phases.needs_user",
    );
    mocks.fetch.mockResolvedValue(
      response([
        { ...waiting, phase: "queued", updated_at: "2026-10-10T05:00:00Z" },
        latest,
        middle,
      ]),
    );
    await act(async () => vi.advanceTimersByTime(2000));
    expect(times()).toEqual([latest.created_at, middle.created_at, waiting.created_at]);
    expect(host.querySelector("ul li:last-child")?.textContent).toContain(
      "voiceSettings.phases.queued",
    );
    expect(mocks.cancel).not.toHaveBeenCalled();
    expect(mocks.retry).not.toHaveBeenCalled();
  });
  it("does not apply an old action after navigating A to B to A", async () => {
    let complete!: (value: unknown) => void;
    mocks.cancel.mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    await render();
    await act(async () => button("voiceSettings.cancelTask").click());
    mocks.fetch.mockResolvedValue(response([]));
    await render("B");
    await render("A");
    await act(async () => complete({ ok: false, error: { message: "Stale cancellation error" } }));
    expect(host.textContent).not.toContain("Stale cancellation error");
    expect(host.querySelector("[data-secretary-tasks]")).toBeNull();
    expect(mocks.cancel).toHaveBeenCalledWith("A", "a-task");
  });
  it("pauses stale polling while a mutation is pending, then refreshes its result", async () => {
    let complete!: (value: unknown) => void;
    mocks.cancel.mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    await render();
    await act(async () => button("voiceSettings.cancelTask").click());
    const reads = mocks.fetch.mock.calls.length;
    await act(async () => vi.advanceTimersByTime(4000));
    expect(mocks.fetch).toHaveBeenCalledTimes(reads);
    mocks.fetch.mockResolvedValue(response([{ ...task, phase: "cancelled" }]));
    await act(async () => complete({ ok: true, result: { cancel_requested: true } }));
    expect(host.textContent).toContain("voiceSettings.phases.cancelled");
    expect(button("voiceSettings.retryTask").disabled).toBe(false);
    expect(mocks.retry).not.toHaveBeenCalled();
  });
  it("releases controls when a visit closes without letting its response clear a newer action", async () => {
    const complete: Array<(value: unknown) => void> = [];
    mocks.cancel.mockImplementation(() => new Promise((resolve) => complete.push(resolve)));
    await render();
    await act(async () => button("voiceSettings.cancelTask").click());
    expect(button("voiceSettings.cancelTask").disabled).toBe(true);
    await render("A", false);
    await render();
    expect(button("voiceSettings.cancelTask").disabled).toBe(false);
    await act(async () => button("voiceSettings.cancelTask").click());
    await act(async () => complete[0]({ ok: false, error: { message: "Departed visit error" } }));
    expect(host.textContent).not.toContain("Departed visit error");
    expect(button("voiceSettings.cancelTask").disabled).toBe(true);
    mocks.fetch.mockResolvedValue(response([{ ...task, phase: "cancelled" }]));
    await act(async () => complete[1]({ ok: true, result: { cancel_requested: true } }));
    expect(button("voiceSettings.retryTask").disabled).toBe(false);
  });
  it("publishes slow task reads without starting overlapping polls", async () => {
    mocks.fetch.mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve(response([task])), 2500)),
    );
    await render();
    await act(async () => vi.advanceTimersByTimeAsync(2500));
    expect(host.textContent).toContain("A input");
    expect(mocks.fetch).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTimeAsync(4000));
    expect(mocks.fetch).toHaveBeenCalledTimes(2);
    expect(host.textContent).toContain("A input");
    expect(mocks.cancel).not.toHaveBeenCalled();
  });
  it("requires clarification before creating a successor in the original Group", async () => {
    mocks.fetch.mockResolvedValue(
      response([
        {
          ...task,
          phase: "needs_user",
          receipt: { status: "needs_user", output: { reply_text: "Which date?" } },
        },
      ]),
    );
    await render();
    expect(host.textContent).toContain("Which date?");
    expect(button("voiceSettings.continueTask").disabled).toBe(true);
    const field = host.querySelector<HTMLTextAreaElement>("textarea")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        field,
        "October 6",
      );
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    mocks.retry.mockResolvedValue({ ok: false, error: { message: "Target was removed" } });
    await act(async () => button("voiceSettings.continueTask").click());
    expect(mocks.retry).toHaveBeenCalledWith("A", "a-task", "October 6", false);
    expect(field.value).toBe("October 6");
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.textContent).toContain("Target was removed");
  });
  it("clears a recovered read error without suppressing later action errors", async () => {
    mocks.fetch.mockResolvedValueOnce({ ok: false, error: { message: "Temporary read failure" } });
    await render();
    expect(host.textContent).toContain("Temporary read failure");
    await act(async () => vi.advanceTimersByTime(2000));
    expect(host.textContent).not.toContain("Temporary read failure");
    expect(host.textContent).toContain("A input");
  });
  it("shows saved input waiting for configuration even before any job exists", async () => {
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [],
        configured: false,
        ready: false,
        readiness_code: "not_configured",
        readiness_error: "Global Voice Secretary is not configured",
        deferred_sources: 3,
      },
    });
    await render();
    expect(host.textContent).toContain("voiceSettings.sourcesUnconfigured");
    expect(host.querySelector("[data-secretary-tasks]")).not.toBeNull();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });
  it("keeps saved input waiting for an unavailable owner distinct from missing configuration", async () => {
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [],
        configured: true,
        ready: false,
        readiness_code: "owner_unavailable",
        readiness_error: "Owner is unavailable",
        deferred_sources: 3,
      },
    });
    await render();
    expect(host.textContent).toContain("voiceSettings.sourcesUnavailable");
    expect(host.textContent).not.toContain("voiceSettings.sourcesUnconfigured");
    expect(mocks.retry).not.toHaveBeenCalled();
    expect(mocks.cancel).not.toHaveBeenCalled();
  });
  it("keeps document gaps and shutdown interruption visible beyond twelve records", async () => {
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        configured: true,
        ready: true,
        readiness_code: null,
        readiness_error: null,
        unprocessed_document_sources: 1,
        tasks: Array.from({ length: 15 }, (_, index) => ({
          ...task,
          task_id: `task-${index}`,
          preview: `Source ${index}`,
          phase: "cancelled",
          cancellation_reason: "shutdown",
          target: { ...task.target, kind: "document" },
        })),
      },
    });
    await render();
    expect(host.textContent).toContain("voiceSettings.sourcesUnprocessed");
    expect(host.textContent).toContain("voiceSettings.interrupted");
    expect(host.textContent).toContain("Source 14");
    expect(host.querySelectorAll("details")).toHaveLength(15);
  });
});

describe("secretary task status line", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    for (const mock of [mocks.fetch, mocks.cancel, mocks.retry, mocks.terminal]) mock.mockReset();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps the line to the requested kinds and shows the next problem without expanding", async () => {
    mocks.fetch.mockResolvedValue(
      response([
        { ...task, task_id: "ask-task", preview: "Ask input", phase: "running" },
        {
          ...task,
          task_id: "doc-failed",
          preview: "Doc input",
          phase: "failed",
          diagnostic: "Document lock was lost",
          target: { ...task.target, kind: "document", document_path: "notes.md" },
        },
        {
          ...task,
          task_id: "doc-done",
          preview: "Done input",
          phase: "done",
          target: { ...task.target, kind: "document", document_path: "done.md" },
        },
      ]),
    );
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} kinds={["document"]} />),
    );
    const line = host.querySelector<HTMLButtonElement>("[data-secretary-tasks] > button")!;
    expect(line.textContent).toContain("voiceSettings.taskLine.attention");
    expect(line.getAttribute("aria-expanded")).toBe("false");
    expect(host.textContent).not.toContain("Ask input");
    const visible = Array.from(host.querySelectorAll("details")).filter(
      (details) => !details.closest("[hidden]"),
    );
    expect(visible).toHaveLength(1);
    expect(visible[0].open).toBe(true);
    expect(visible[0].textContent).toContain("Document lock was lost");
    expect(host.querySelectorAll("details")).toHaveLength(2);
    await act(async () => line.click());
    expect(line.getAttribute("aria-expanded")).toBe("true");
    expect(host.querySelectorAll("details")).toHaveLength(2);
  });

  it("leaves readiness errors to the workspace banner when asked", async () => {
    mocks.fetch.mockResolvedValue({
      ok: true,
      result: {
        tasks: [],
        configured: false,
        ready: false,
        readiness_code: "not_configured",
        readiness_error: "Pick a runtime",
        deferred_sources: 1,
      },
    });
    await act(async () =>
      root.render(<SecretaryTasks groupId="A" active isDark={false} hideReadinessError />),
    );
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.textContent).toContain("voiceSettings.sourcesUnconfigured");
  });

  it("opens execution details on the task chosen from a row", async () => {
    mocks.fetch.mockResolvedValue(
      response([
        { ...task, task_id: "first", preview: "First", phase: "running" },
        { ...task, task_id: "second", preview: "Second", phase: "done" },
      ]),
    );
    await act(async () =>
      root.render(
        <SecretaryTasks groupId="A" active isDark={false} workspace initialTaskId="second" />,
      ),
    );
    expect(host.querySelector("[data-secretary-stage] h4")?.textContent).toContain("Second");
  });
});

it("shows held and invalid saved sources even when no task exists", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mocks.fetch.mockResolvedValue({
    ok: true,
    result: {
      tasks: [],
      configured: true,
      ready: true,
      readiness_code: null,
      readiness_error: null,
      held_sources: 4,
      invalid_sources: 1,
    },
  });
  await act(async () => root.render(<SecretaryTasks groupId="A" active isDark={false} />));
  expect(host.textContent).toContain("voiceSettings.sourcesHeld");
  expect(host.textContent).toContain("voiceSettings.sourcesInvalid");
  await act(async () => root.unmount());
  host.remove();
});

describe("execution result delivery labels", () => {
  it("does not infer document writes or synchronized answers from completion alone", () => {
    const done = { ...task, phase: "done" as const };
    expect(secretaryTaskOutcome(done)).toBe("completed");
    expect(secretaryTaskOutcome({ ...done, projected_at: "2026-10-09T01:00:00Z" })).toBe(
      "answerReady",
    );
    const doc = { ...done, target: { ...task.target, kind: "document" as const } };
    expect(secretaryTaskOutcome(doc)).toBe("completed");
    expect(
      secretaryTaskOutcome({
        ...doc,
        receipt: { status: "done", document_version: "sha", output: {} },
      }),
    ).toBe("documentWritten");
    expect(
      secretaryTaskOutcome({ ...doc, projection_error: "Index could not be synchronized" }),
    ).toBe("syncFailed");
  });
  it("distinguishes unchanged, stale and dismissed prompt results", () => {
    const prompt = {
      ...task,
      phase: "done" as const,
      target: { ...task.target, kind: "prompt" as const },
    };
    expect(
      secretaryTaskOutcome({ ...prompt, receipt: { status: "done", output: { no_op: true } } }),
    ).toBe("noChange");
    expect(secretaryTaskOutcome({ ...prompt, prompt_draft_status: "stale" })).toBe("stale");
    expect(secretaryTaskOutcome({ ...prompt, prompt_draft_status: "dismissed" })).toBe("dismissed");
  });
});
