// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceAnalystInfo } from "../../services/api/codexVoice";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { controlCodexVoiceAnalyst, fetchActiveCodexVoiceCall } from "../../services/api/codexVoice";
import { CodexVoiceAnalystModal } from "./CodexVoiceAnalystModal";

const backend = vi.hoisted(() => ({ analyst: null as CodexVoiceAnalystInfo | null }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../hooks/useModalA11y", () => ({
  useModalA11y: () => ({ modalRef: createRef<HTMLDivElement>() }),
}));
vi.mock("../../features/codexVoice/CodexVoiceMessageSources", () => ({
  CodexVoiceMessageSources: () => null,
}));
vi.mock("../../features/codexVoice/VoiceAnalystTerminal", () => ({
  VoiceAnalystTerminal: () => <div>native-terminal</div>,
}));
vi.mock("../../services/api/codexVoice", () => ({
  fetchActiveCodexVoiceCall: vi.fn(async () => ({
    ok: true,
    result: { analyst: backend.analyst },
  })),
  controlCodexVoiceAnalyst: vi.fn(async () => ({ ok: true, result: { accepted: true } })),
}));

function fixtureController(): CodexVoiceSessionController {
  return {
    audioDeviceSnapshot: null,
    audioRef: createRef<HTMLAudioElement>(),
    phase: "listening",
    call: null,
    analyst: {
      generation: "fixture-acp",
      structured: true,
      tui_ready: false,
      phase: "working",
      progress: "",
      last_result: "",
      warning: "",
    },
    owned: true,
    checking: false,
    conversation: [],
    notificationPaused: false,
    microphoneMuted: false,
    playbackBlocked: false,
    outputStatus: { queued: 0, blocked: null },
    error: "",
    isStarting: false,
    isEngaged: true,
    externalCall: false,
    analystWorking: true,
    analystWarning: "",
    preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
    supportedVoices: ["cove"],
    readiness: {
      analyst_runtime: "antigravity",
      analyst_runtime_available: true,
      supported_modes: ["assistant", "persona"],
      realtime_credentials_available: true,
    },
    updatePreferences: vi.fn(),
    updateAnalystSnapshot: vi.fn(),
    refresh: vi.fn(async () => undefined),
    start: vi.fn(async () => undefined),
    disconnect: vi.fn(async () => undefined),
    cancelInvestigation: vi.fn(async () => true),
    toggleMicrophone: vi.fn(),
    resumeAudio: vi.fn(async () => undefined),
    startNewAnalyst: vi.fn(async () => true),
    clearError: vi.fn(),
  };
}

describe("Voice ACP console in the complete modal", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let controller: CodexVoiceSessionController;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    vi.mocked(fetchActiveCodexVoiceCall).mockClear();
    vi.mocked(controlCodexVoiceAnalyst).mockClear();
    controller = fixtureController();
    backend.analyst = {
      ...controller.analyst!,
      permissions: [
        {
          request_id: "0",
          title: "Run search_web?",
          kind: "search",
          details: { query: "Tokyo weather today" },
        },
      ],
    };
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

  function button(label: string): HTMLButtonElement {
    const found = [...host.querySelectorAll("button")].find((item) => item.textContent === label);
    expect(found, label).toBeDefined();
    return found!;
  }

  async function render(desktop = true, isOpen = true, expandAnalyst = desktop) {
    vi.stubGlobal("matchMedia", () => ({
      matches: desktop,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    await act(async () =>
      root.render(
        <CodexVoiceAnalystModal
          isOpen={isOpen}
          isDark={false}
          isSmallScreen={!desktop}
          controller={controller}
          onClose={vi.fn()}
        />,
      ),
    );
    const show = [...host.querySelectorAll("button")].find(
      (item) => item.textContent === "codexVoiceShowAnalyst",
    );
    if (expandAnalyst && isOpen && show) await act(async () => show.click());
  }

  it("keeps a collapsed Analyst summary current during its call", async () => {
    controller.call = {
      generation: "call",
      analyst_generation: "fixture-acp",
      mode: "assistant",
      connected: true,
    } as never;
    controller.analyst = { ...controller.analyst!, permissions: backend.analyst!.permissions };
    await render(true, true, false);
    const bar = host.querySelector("[data-codex-voice-analyst-bar]");
    expect(bar?.textContent).toContain("codexVoiceAnalystNeedsInput");
    expect(bar?.textContent).toContain("Antigravity");
    expect(host.querySelector("#codex-voice-analyst-pane")?.parentElement?.className).toBe(
      "hidden",
    );
    expect(fetchActiveCodexVoiceCall).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[aria-live="polite"]')?.textContent).toBe("codexVoiceInCall");

    await act(async () => button("codexVoiceShowAnalyst").click());
    expect(host.querySelector("[data-codex-voice-analyst-bar]")).toBeNull();
    expect(fetchActiveCodexVoiceCall).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("Run search_web?");
  });

  it("marks the narrow Analyst tab without switching to it", async () => {
    controller.analyst = { ...controller.analyst!, permissions: backend.analyst!.permissions };
    await render(false);
    const tab = button("codexVoiceAnalystTitle");
    expect(tab.getAttribute("aria-pressed")).toBe("false");
    expect(tab.querySelector("[data-codex-voice-analyst-dot]")).not.toBeNull();
    expect(tab.getAttribute("aria-label")).toBe(
      "codexVoiceAnalystTitle · codexVoiceAnalystHasUpdates",
    );
    await act(async () => tab.click());
    expect(tab.querySelector("[data-codex-voice-analyst-dot]")).toBeNull();
  });

  it("fetches a pending ACP search permission without a native TUI, then displays its result", async () => {
    await render();
    expect(fetchActiveCodexVoiceCall).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("Run search_web?");
    expect(host.textContent).not.toContain("native-terminal");
    expect(controlCodexVoiceAnalyst).not.toHaveBeenCalled();

    await act(async () => button("acpControls.allow").click());
    expect(controlCodexVoiceAnalyst).toHaveBeenCalledExactlyOnceWith("fixture-acp", {
      action: "permission",
      request_id: "0",
      allow: true,
    });
    backend.analyst = {
      ...backend.analyst!,
      permissions: [],
      phase: "ready",
      last_result: "Fixture weather result",
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.textContent).toContain("Fixture weather result");
    expect(host.textContent).not.toContain("Run search_web?");
  });

  it("pauses reads when collapsed or closed and refreshes when shown again", async () => {
    controller.call = null;
    controller.owned = false;
    await render(true, true, false);
    expect(fetchActiveCodexVoiceCall).not.toHaveBeenCalled();
    await act(async () => button("codexVoiceShowAnalyst").click());
    expect(fetchActiveCodexVoiceCall).toHaveBeenCalledTimes(1);
    await act(async () => button("codexVoiceHideAnalyst").click());
    vi.mocked(fetchActiveCodexVoiceCall).mockClear();
    await act(async () => vi.advanceTimersByTimeAsync(2400));
    expect(fetchActiveCodexVoiceCall).not.toHaveBeenCalled();
    await act(async () => button("codexVoiceShowAnalyst").click());
    expect(fetchActiveCodexVoiceCall).toHaveBeenCalledTimes(1);
    await render(true, false);
    vi.mocked(fetchActiveCodexVoiceCall).mockClear();
    await act(async () => vi.advanceTimersByTimeAsync(2400));
    expect(fetchActiveCodexVoiceCall).not.toHaveBeenCalled();
    await render();
    expect(fetchActiveCodexVoiceCall).toHaveBeenCalledTimes(1);
  });

  it("waits for the Analyst tab on narrow screens and keeps Deny request-scoped", async () => {
    controller.call = null;
    controller.owned = false;
    await render(false);
    expect(fetchActiveCodexVoiceCall).not.toHaveBeenCalled();
    await act(async () => button("codexVoiceAnalystTitle").click());
    expect(fetchActiveCodexVoiceCall).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("Run search_web?");
    await act(async () => button("acpControls.deny").click());
    expect(controlCodexVoiceAnalyst).toHaveBeenCalledExactlyOnceWith("fixture-acp", {
      action: "permission",
      request_id: "0",
      allow: false,
    });
    await act(async () => button("codexVoiceConversation").click());
    vi.mocked(fetchActiveCodexVoiceCall).mockClear();
    await act(async () => vi.advanceTimersByTimeAsync(2400));
    expect(fetchActiveCodexVoiceCall).not.toHaveBeenCalled();
  });
  it("shows failures after queue acceptance, preserves partial output, and clears old errors", async () => {
    controller.isEngaged = false;
    controller.owned = false;
    controller.phase = "idle";
    backend.analyst = { ...backend.analyst!, permissions: [] };
    await render();
    backend.analyst = {
      ...backend.analyst!,
      phase: "needs_attention",
      last_error: "Synthetic admission failure",
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(
      "Synthetic admission failure",
    );
    expect(host.textContent).toContain("acpControls.turnFailed");
    expect(host.textContent).not.toContain("acpControls.partialOutput");
    backend.analyst = {
      ...backend.analyst!,
      last_error: "Synthetic provider failure",
      last_result: "Partial weather lookup",
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.textContent).toContain("Synthetic provider failure");
    expect(host.textContent).toContain("acpControls.partialOutput");
    expect(host.textContent).toContain("Partial weather lookup");
    backend.analyst = {
      ...backend.analyst!,
      phase: "working",
      last_error: "",
      last_result: "",
      progress: "New investigation",
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.textContent).not.toContain("Partial weather lookup");
    backend.analyst = {
      ...backend.analyst!,
      phase: "ready",
      progress: "",
      last_result: "Completed answer",
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.textContent).toContain("Completed answer");
  });
  it("keeps ACP task failure details in the output pane and retains disconnection warnings", async () => {
    controller.analyst = {
      ...controller.analyst!,
      phase: "needs_attention",
      warning: "analyst_turn_failed",
      last_error: "Synthetic provider failure",
    };
    controller.analystWarning = "Task failure summary";
    backend.analyst = controller.analyst;
    await render();
    expect(host.textContent).toContain("Synthetic provider failure");
    expect(host.textContent).not.toContain("Task failure summary");
    controller.analyst = { ...controller.analyst, warning: "analyst_disconnected" };
    controller.analystWarning = "Analyst disconnected";
    backend.analyst = controller.analyst;
    await render();
    expect(host.textContent).toContain("Analyst disconnected");
    expect(host.textContent).toContain("Synthetic provider failure");
  });
  async function fill(value: string) {
    const field = host.querySelector("textarea")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        field,
        value,
      );
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    return field;
  }

  function ownCall(generation: string) {
    controller.call = {
      generation,
      analyst_generation: "fixture-acp",
      mode: "assistant",
      voice: "cove",
      connected: true,
    };
    controller.owned = true;
  }

  it("explicitly binds an investigation to this owned call and shows its paired outcome", async () => {
    ownCall("call-a");
    backend.analyst = { ...backend.analyst!, permissions: [] };
    await render();
    expect(host.textContent).toContain("acpControls.currentCallHint");
    const field = await fill("武汉天气\n请保留来源");
    await act(async () => button("acpControls.submitInvestigation").click());
    const [generation, command] = vi.mocked(controlCodexVoiceAnalyst).mock.calls.at(-1)!;
    expect(generation).toBe("fixture-acp");
    expect(command).toMatchObject({
      action: "input",
      text: "武汉天气\n请保留来源",
      call_generation: "call-a",
    });
    expect(field.value).toBe("");
    backend.analyst = {
      ...backend.analyst!,
      manual_task_id: "manual-input:one",
      last_result: "调查结果",
      manual_tasks: [
        {
          id: "manual-input:one",
          text: "武汉天气\n请保留来源",
          call_generation: "call-a",
          status: "completed",
          result: "调查结果",
          error: "",
        },
      ],
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.textContent).toContain("武汉天气");
    expect(host.textContent!.match(/调查结果/g)).toHaveLength(1);
    expect(host.textContent).toContain("acpControls.callResult");
    ownCall("call-b");
    await render();
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.textContent).toContain("acpControls.endedCallResult");
  });

  it("preserves the original task ID and call on a failed request, including keyboard retry", async () => {
    ownCall("call-a");
    await render();
    const failure = {
      ok: false,
      error: { code: "fixture_error", message: "Synthetic control failure" },
    };
    vi.mocked(controlCodexVoiceAnalyst).mockResolvedValueOnce(failure as never);
    const field = await fill("fixture investigation");
    await act(async () => button("acpControls.submitInvestigation").click());
    const first = vi.mocked(controlCodexVoiceAnalyst).mock.calls.at(-1)![1];
    expect(field.value).toBe("fixture investigation");
    ownCall("call-b");
    await render();
    vi.mocked(controlCodexVoiceAnalyst).mockResolvedValueOnce(failure as never);
    await act(async () =>
      field.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true }),
      ),
    );
    expect(vi.mocked(controlCodexVoiceAnalyst).mock.calls.at(-1)![1]).toEqual(first);
    await fill("explicit new investigation");
    await act(async () => button("acpControls.submitInvestigation").click());
    const last = vi.mocked(controlCodexVoiceAnalyst).mock.calls.at(-1)![1];
    expect(last).toMatchObject({ text: "explicit new investigation", call_generation: "call-b" });
    if (last.action === "input" && first.action === "input")
      expect(last.input_id).not.toBe(first.input_id);
  });

  it("keeps standalone and foreign/persona call input local; Enter and IME do not submit", async () => {
    controller.owned = false;
    ownCall("foreign");
    controller.owned = false;
    await render();
    expect(host.textContent).toContain("acpControls.localHint");
    const field = await fill("local investigation");
    await act(async () => {
      field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      field.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          ctrlKey: true,
          isComposing: true,
          bubbles: true,
        }),
      );
    });
    expect(controlCodexVoiceAnalyst).not.toHaveBeenCalled();
    await act(async () => button("acpControls.submitInvestigation").click());
    expect(vi.mocked(controlCodexVoiceAnalyst).mock.calls.at(-1)![1]).toMatchObject({
      call_generation: null,
    });
    ownCall("persona");
    controller.call = { ...controller.call!, mode: "persona", analyst_generation: null };
    await render();
    expect(host.textContent).toContain("acpControls.localHint");
  });

  it("shows a failed investigation once with its partial result and original task", async () => {
    backend.analyst = {
      ...backend.analyst!,
      permissions: [],
      manual_task_id: "failed-one",
      last_error: "Synthetic task failure",
      last_result: "Partial result",
      manual_tasks: [
        {
          id: "failed-one",
          text: "original investigation",
          call_generation: null,
          status: "failed",
          result: "Partial result",
          error: "Synthetic task failure",
        },
      ],
    };
    await render();
    expect(host.querySelectorAll('[role="alert"]')).toHaveLength(1);
    expect(host.textContent).toContain("original investigation");
    expect(host.textContent).toContain("Partial result");
    expect(host.textContent).toContain("acpControls.partialOutput");
    expect(host.textContent).toContain("acpControls.localResult");
    expect(host.textContent!.indexOf("acpControls.partialOutput")).toBeLessThan(
      host.textContent!.indexOf("Partial result"),
    );
    backend.analyst = {
      ...backend.analyst,
      manual_tasks: [
        { ...backend.analyst.manual_tasks![0], status: "unconfirmed", error: "Disconnected" },
      ],
    };
    await act(async () => vi.advanceTimersByTimeAsync(1200));
    expect(host.textContent).toContain("acpControls.taskStatus.unconfirmed");
    expect(host.textContent).toContain("acpControls.outcomeUnconfirmed");
    expect(host.textContent).not.toContain("acpControls.turnFailed");
  });
  it("does not let an older GET or socket snapshot erase an accepted task", async () => {
    let releaseRead!: (value: unknown) => void;
    vi.mocked(fetchActiveCodexVoiceCall).mockReturnValueOnce(
      new Promise((resolve) => {
        releaseRead = resolve as (value: unknown) => void;
      }) as never,
    );
    ownCall("call-a");
    await render();
    const accepted = {
      ...controller.analyst!,
      manual_task_id: null,
      manual_tasks: [
        {
          id: "accepted",
          text: "Accepted task",
          call_generation: "call-a",
          status: "queued",
          result: "",
          error: "",
        },
      ],
    };
    vi.mocked(controlCodexVoiceAnalyst).mockResolvedValueOnce({
      ok: true,
      result: { accepted: true, analyst: accepted },
    });
    await fill("Accepted task");
    await act(async () => button("acpControls.submitInvestigation").click());
    expect(host.textContent).toContain("Accepted task");
    await act(async () =>
      releaseRead({ ok: true, result: { analyst: { ...controller.analyst, manual_tasks: [] } } }),
    );
    expect(host.textContent).toContain("Accepted task");
    controller.analyst = { ...controller.analyst!, manual_tasks: [], phase: "working" };
    await render();
    expect(host.textContent).toContain("Accepted task");
    backend.analyst = accepted;
  });
});
