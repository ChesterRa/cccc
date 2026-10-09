// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { TFunction } from "i18next";
import type { AssistantVoiceAskFeedback, SecretaryTaskSummary } from "../../../types";
import type { SecretaryTasksController } from "./useSecretaryTasks";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../../components/LazyMarkdownRenderer", () => ({
  LazyMarkdownRenderer: ({ content }: { content: string }) => <strong>{content}</strong>,
}));
import { VoiceAskThread } from "./VoiceAskThread";

const t = ((key: string) => key) as unknown as TFunction;
const task = (patch: Partial<SecretaryTaskSummary>): SecretaryTaskSummary => ({
  task_id: "task-a",
  target: { group_id: "A", scope_key: "s", kind: "ask", document_path: "", request_id: "req-a" },
  phase: "running",
  cleanup_confirmed: false,
  created_at: "",
  updated_at: "",
  source_count: 1,
  preview: "",
  diagnostic: "",
  projection_error: "",
  previous_task_id: "",
  superseded_by: "",
  candidate_available: false,
  ...patch,
});
const controller = (tasks: SecretaryTaskSummary[]) =>
  ({
    tasks,
    coverage: { deferred: 0, held: 0, invalid: 0, unprocessed: 0, configured: true },
    error: "",
    loadError: "",
    busy: "",
    followups: {},
    setFollowup: vi.fn(),
    candidate: null,
    copyCandidate: vi.fn(),
    act: vi.fn(),
  }) as unknown as SecretaryTasksController;

describe("VoiceAskThread", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  const render = (
    items: AssistantVoiceAskFeedback[],
    tasks: SecretaryTasksController,
    handlers: { onFollowUp?: () => void; onShowExecution?: () => void } = {},
  ) =>
    act(async () =>
      root.render(
        <VoiceAskThread
          items={items.map((item, index) => ({ item, sortAt: index + 1, status: item.status }))}
          isDark={false}
          t={t}
          documents={[]}
          documentKey={() => ""}
          tasks={tasks}
          statusLabel={(status) => `status:${status}`}
          statusClassName={() => ""}
          formatTime={() => ""}
          formatFullTime={() => ""}
          onOpenDocument={vi.fn()}
          onFollowUp={handlers.onFollowUp || vi.fn()}
          onCopyReply={vi.fn()}
          onShowExecution={handlers.onShowExecution || vi.fn()}
          onClear={vi.fn()}
          clearing={false}
        />,
      ),
    );
  const button = (label: string) =>
    Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (item) => item.textContent === label,
    );

  it("renders the reply as Markdown and lets the user follow up on it", async () => {
    const onFollowUp = vi.fn();
    await render(
      [{ request_id: "req-a", status: "done", request_text: "Why?", reply_text: "**Because**" }],
      controller([]),
      { onFollowUp },
    );
    expect(host.querySelector("strong")?.textContent).toBe("**Because**");
    await act(async () => button("voiceSecretaryFollowUp")!.click());
    expect(onFollowUp).toHaveBeenCalledWith(expect.objectContaining({ request_id: "req-a" }));
  });

  it("binds cancel and execution details to the task with the same request id", async () => {
    const tasks = controller([
      task({}),
      task({ task_id: "other", target: { ...task({}).target, request_id: "req-b" } }),
    ]);
    const onShowExecution = vi.fn();
    await render([{ request_id: "req-a", status: "working", request_text: "Check" }], tasks, {
      onShowExecution,
    });
    await act(async () => button("settings:voiceSettings.cancelTask")!.click());
    expect(tasks.act).toHaveBeenCalledWith(
      expect.objectContaining({ task_id: "task-a" }),
      "cancel",
    );
    await act(async () => button("settings:voiceSettings.viewExecution")!.click());
    expect(onShowExecution).toHaveBeenCalledWith(expect.objectContaining({ task_id: "task-a" }));
  });

  it("shows the action needed for a failed task inline", async () => {
    await render(
      [{ request_id: "req-a", status: "failed", request_text: "Check" }],
      controller([
        task({ phase: "failed", cleanup_confirmed: true, diagnostic: "Runtime exited" }),
      ]),
    );
    const details = host.querySelector("details")!;
    expect(details.open).toBe(true);
    expect(details.textContent).toContain("Runtime exited");
    expect(button("voiceSettings.retryTask")).toBeTruthy();
  });
});
