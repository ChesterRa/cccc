// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { CodexVoiceAnalystModal } from "../../components/modals/CodexVoiceAnalystModal";
import { useCodexVoiceSessionController } from "./useCodexVoiceSessionController";

const fixture = vi.hoisted(() => ({
  active: vi.fn(),
  cancel: vi.fn(),
  control: vi.fn(),
  sessions: [] as Array<{
    callbacks: {
      onPhase(value: string): void;
      onCall(value: unknown): void;
      onError(code: string): void;
      onAnalyst(value: unknown): void;
    };
    cancel: () => boolean;
  }>,
  t: (key: string) => key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: fixture.t }) }));
vi.mock("../../services/api", () => ({
  fetchActiveCodexVoiceCall: fixture.active,
  cancelCodexVoiceAnalyst: fixture.cancel,
  resetCodexVoiceAnalyst: vi.fn(),
  stopCodexVoiceCall: vi.fn(),
}));
vi.mock("../../services/api/codexVoice", () => ({
  fetchActiveCodexVoiceCall: fixture.active,
  controlCodexVoiceAnalyst: fixture.control,
}));
vi.mock("./CodexVoiceMessageSources", () => ({ CodexVoiceMessageSources: () => null }));
vi.mock("./VoiceAnalystTerminal", () => ({ VoiceAnalystTerminal: () => null }));
vi.mock("./codexVoiceSession", () => ({
  CodexVoiceBrowserSession: class {
    options: (typeof fixture.sessions)[number];
    constructor(options: (typeof fixture.sessions)[number]) {
      this.options = { ...options, cancel: vi.fn(() => true) };
      fixture.sessions.push(this.options);
    }
    async start() {
      this.options.callbacks.onPhase("listening");
      this.options.callbacks.onCall({ generation: "owned", analyst_generation: "analyst" });
    }
    async stop() {
      this.options.callbacks.onCall(null);
      this.options.callbacks.onPhase("idle");
    }
    cancelInvestigation() {
      return this.options.cancel();
    }
    updateAnalystSnapshot(value: unknown) {
      this.options.callbacks.onAnalyst(value);
    }
    audioPreferences() {
      return null;
    }
  },
}));

function snapshot(
  result = "last result",
  structured = false,
  phase: "working" | "ready" = "working",
) {
  return {
    ok: true as const,
    result: {
      call: null,
      analyst: {
        generation: "analyst",
        structured,
        tui_ready: !structured,
        phase,
        last_result: result,
        warning: "",
      },
      voices: ["cove"],
      readiness: {
        analyst_runtime: structured ? "copilot" : "codex",
        analyst_runtime_available: true,
        realtime_credentials_available: true,
      },
    },
  };
}
const failure = { ok: false, error: { code: "NETWORK_ERROR", message: "fixture read failed" } };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
let host: HTMLDivElement;
let root: Root;
let current!: ReturnType<typeof useCodexVoiceSessionController>;
function App() {
  current = useCodexVoiceSessionController();
  return (
    <>
      <audio ref={current.audioRef} />
      <CodexVoiceAnalystModal
        isOpen
        isDark={false}
        isSmallScreen={false}
        controller={current}
        onClose={() => {}}
      />
    </>
  );
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  fixture.active.mockReset().mockResolvedValue(snapshot());
  fixture.cancel.mockReset().mockResolvedValue({ ok: true, result: { cancelled: true } });
  fixture.control.mockReset();
  fixture.sessions.length = 0;
  vi.stubGlobal("matchMedia", () => ({
    matches: true,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const mount = () => act(async () => root.render(<App />));

it("reports an initial status read failure without inventing an execution error or terminal action", async () => {
  fixture.active.mockResolvedValue(failure);
  await mount();
  expect(current.error).toBe("");
  expect(current.refreshError).toBe("codexVoiceStatusUnavailable");
  expect(host.textContent).toContain("codexVoiceStatusUnavailable");
  expect(host.querySelector("[data-codex-voice-status-read] button")).not.toBeNull();
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(fixture.sessions).toHaveLength(0);
});

it("retains a snapshot on a failed read and clears only that failure after recovery", async () => {
  await mount();
  fixture.active.mockResolvedValue(failure);
  await act(async () => current.refresh(false));
  expect(current.analyst?.last_result).toBe("last result");
  expect(current.error).toBe("");
  expect(current.refreshError).toBe("codexVoiceStatusStale");
  fixture.active.mockResolvedValue(snapshot("recovered"));
  await act(async () => current.refresh(false));
  expect(current.analyst?.last_result).toBe("recovered");
  expect(current.refreshError).toBe("");
});

it("does not clear a genuine cancel error when a status read succeeds", async () => {
  await mount();
  fixture.cancel.mockResolvedValue({
    ok: false,
    error: { code: "analyst_turn_failed", message: "fixture cancel failed" },
  });
  await act(async () => current.cancelInvestigation());
  const operationError = current.error;
  expect(operationError).not.toBe("");
  await act(async () => current.refresh(false));
  expect(current.error).toBe(operationError);
});

it("publishes slow reads during continuous focus and coalesces one subsequent read", async () => {
  let concurrent = 0;
  let maximum = 0;
  let reads = 0;
  fixture.active.mockImplementation(
    () =>
      new Promise((resolve) => {
        concurrent += 1;
        maximum = Math.max(maximum, concurrent);
        const result = snapshot(`snapshot ${++reads}`, false, "ready");
        setTimeout(() => {
          concurrent -= 1;
          resolve(result);
        }, 2500);
      }),
  );
  await mount();
  await act(async () => {
    for (let i = 0; i < 2; i++) {
      await vi.advanceTimersByTimeAsync(1000);
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
    }
    await vi.advanceTimersByTimeAsync(500);
  });
  expect(current.analyst?.last_result).toBe("snapshot 1");
  expect(fixture.active).toHaveBeenCalledTimes(2);
  expect(maximum).toBe(1);
  await act(async () => vi.advanceTimersByTimeAsync(2500));
  expect(current.analyst?.last_result).toBe("snapshot 2");
  expect(fixture.active).toHaveBeenCalledTimes(2);
});

it("retires a queued refresh when a call starts and never reads over its owned socket", async () => {
  await mount();
  const pending = deferred<ReturnType<typeof snapshot>>();
  fixture.active.mockReturnValue(pending.promise);
  await act(async () => {
    void current.refresh(false);
    window.dispatchEvent(new Event("focus"));
  });
  await act(async () => current.start());
  expect(current.owned).toBe(true);
  await act(async () => {
    pending.resolve(snapshot("obsolete"));
  });
  expect(current.analyst?.last_result).toBe("last result");
  expect(fixture.active).toHaveBeenCalledTimes(2);
  await act(async () => {
    await current.refresh(false);
    await current.cancelInvestigation();
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(6000);
  });
  expect(fixture.active).toHaveBeenCalledTimes(2);
  expect(fixture.sessions[0].cancel).toHaveBeenCalledOnce();
});

it("retires pending reads and their queued follow-up when an investigation is cancelled", async () => {
  await mount();
  const pending = deferred<ReturnType<typeof snapshot>>();
  fixture.active.mockReturnValue(pending.promise);
  await act(async () => {
    void current.refresh(false);
    window.dispatchEvent(new Event("focus"));
  });
  await act(async () => current.cancelInvestigation());
  await act(async () => pending.resolve(snapshot("obsolete cancellation snapshot")));
  expect(current.analyst?.last_result).toBe("last result");
  expect(fixture.active).toHaveBeenCalledTimes(2);
  expect(fixture.cancel).toHaveBeenCalledWith("analyst");
});

it("shares the same in-flight read when scheduled polling coincides with a focus refresh", async () => {
  await mount();
  const pending = deferred<ReturnType<typeof snapshot>>();
  fixture.active.mockReturnValueOnce(pending.promise);
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await act(async () => vi.advanceTimersByTimeAsync(1500));
  expect(fixture.active).toHaveBeenCalledTimes(2);
  fixture.active.mockResolvedValue(snapshot("latest"));
  await act(async () => pending.resolve(snapshot("slow snapshot")));
  expect(fixture.active).toHaveBeenCalledTimes(3);
  expect(current.analyst?.last_result).toBe("latest");
});

it("keeps an ACP status read failure in one place while the last result remains available", async () => {
  fixture.active.mockResolvedValue(snapshot("ACP last result", true));
  await mount();
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent === "codexVoiceShowAnalyst")!
      .click(),
  );
  fixture.active.mockResolvedValue(failure);
  await act(async () => {
    await current.refresh(false);
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(current.error).toBe("");
  expect(host.querySelectorAll("[data-codex-voice-status-read]")).toHaveLength(1);
  expect(host.querySelectorAll("[data-acp-snapshot-stale]")).toHaveLength(0);
  expect(host.textContent).toContain("ACP last result");
});

it("keeps the ACP console's independent freshness hint during an owned call", async () => {
  fixture.active.mockResolvedValue(snapshot("ACP last result", true));
  await mount();
  await act(async () => current.start());
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent === "codexVoiceShowAnalyst")!
      .click(),
  );
  fixture.active.mockResolvedValue(failure);
  await act(async () => vi.advanceTimersByTimeAsync(2500));
  expect(current.error).toBe("");
  expect(host.querySelectorAll("[data-codex-voice-status-read]")).toHaveLength(0);
  expect(host.querySelectorAll("[data-acp-snapshot-stale]")).toHaveLength(1);
  expect(host.textContent).toContain("ACP last result");
});

it("does not let a slow status read replace the ACP queue accepted by a later control", async () => {
  fixture.active.mockResolvedValue(snapshot("last result", true, "ready"));
  await mount();
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent === "codexVoiceShowAnalyst")!
      .click(),
  );
  const pending = deferred<ReturnType<typeof snapshot>>();
  fixture.active.mockReturnValueOnce(pending.promise);
  await act(async () => {
    void current.refresh(false);
  });
  const updated = {
    ...snapshot("last result", true).result.analyst,
    queued_inputs: 1,
    manual_tasks: [
      {
        id: "accepted",
        text: "accepted investigation",
        status: "queued",
        result: "",
        error: "",
        call_generation: null,
      },
    ],
  };
  fixture.control.mockResolvedValue({ ok: true, result: { accepted: true, analyst: updated } });
  await act(async () => {
    const field = host.querySelector("textarea")!;
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
      field,
      "accepted investigation",
    );
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(current.analyst?.queued_inputs).toBe(1);
  await act(async () => pending.resolve(snapshot("obsolete", true, "ready")));
  expect(current.analyst?.queued_inputs).toBe(1);
  expect(current.analystWorking).toBe(true);
  expect(
    [...host.querySelectorAll("button")].some((b) =>
      b.textContent?.includes("actors:acpControls.cancel"),
    ),
  ).toBe(true);
});

it("allows a slow controller read to finish despite faster ordinary ACP snapshot polling", async () => {
  fixture.active.mockResolvedValue(snapshot("initial", true, "ready"));
  await mount();
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent === "codexVoiceShowAnalyst")!
      .click(),
  );
  const pending = deferred<ReturnType<typeof snapshot>>();
  fixture.active
    .mockReturnValueOnce(pending.promise)
    .mockResolvedValue(snapshot("ACP polling", true, "ready"));
  await act(async () => {
    void current.refresh(false);
  });
  await act(async () => vi.advanceTimersByTimeAsync(2500));
  expect(current.analyst?.last_result).toBe("ACP polling");
  await act(async () => pending.resolve(snapshot("controller completed", true, "ready")));
  expect(current.analyst?.last_result).toBe("controller completed");
});

it("recovers the controller's failed read while ordinary ACP snapshots keep arriving", async () => {
  fixture.active.mockResolvedValue(snapshot("initial", true, "ready"));
  await mount();
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((button) => button.textContent === "codexVoiceShowAnalyst")!
      .click(),
  );
  fixture.active.mockResolvedValue(failure);
  await act(async () => current.refresh(false));
  expect(current.refreshError).toBe("codexVoiceStatusStale");
  fixture.active.mockResolvedValue(snapshot("recovered ACP snapshot", true, "ready"));
  // Each fast read commits its React update before the next tick, as in the
  // browser. One batched timer jump would hide timer-reset starvation.
  for (let i = 0; i < 5; i++) {
    await act(async () => vi.advanceTimersByTimeAsync(1200));
  }
  expect(current.analyst?.last_result).toBe("recovered ACP snapshot");
  expect(current.refreshError).toBe("");
  expect(host.querySelector("[data-codex-voice-status-read]")).toBeNull();
});

it("resumes scheduled controller polling after its owned call ends without an Analyst phase change", async () => {
  await mount();
  await act(async () => current.start());
  await act(async () => current.updateAnalystSnapshot({ ...current.analyst!, phase: "ready" }));
  await act(async () => vi.advanceTimersByTimeAsync(6000));
  expect(fixture.active).toHaveBeenCalledTimes(1);
  await act(async () => current.disconnect());
  await act(async () => vi.advanceTimersByTimeAsync(5000));
  expect(fixture.active).toHaveBeenCalledTimes(2);
});
