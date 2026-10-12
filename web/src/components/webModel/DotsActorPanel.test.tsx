// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { DotsActorState } from "../../services/api/webModel";

const mocks = vi.hoisted(() => ({ load: vi.fn(), update: vi.fn(), copy: vi.fn(), language: "en" }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { time?: string }) =>
      options?.time ? `${key}: ${options.time}` : key,
    i18n: { language: mocks.language },
  }),
}));
vi.mock("../../services/api", () => ({
  fetchDotsActorState: (...args: unknown[]) => mocks.load(...args),
  updateDotsActorConnection: (...args: unknown[]) => mocks.update(...args),
}));
import { DotsActorPanel } from "./DotsActorPanel";

const state = (patch: Partial<DotsActorState> = {}) => ({
  ok: true as const,
  result: {
    group_id: "g_a",
    actor_id: "dot",
    enabled: false,
    connected: false,
    subscribed: false,
    phase: "unconfigured" as const,
    pending_count: 0,
    ...patch,
  },
});
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  vi.clearAllMocks();
  mocks.language = "en";
  mocks.load.mockResolvedValue(state());
  mocks.update.mockResolvedValue({
    ok: true,
    result: {
      connected: true,
      instructions: "Create a native Work event task: cccc.dots.ready; actor_token=synthetic-only",
    },
  });
  vi.stubGlobal("confirm", vi.fn().mockReturnValue(true));
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: mocks.copy.mockResolvedValue(undefined) },
    configurable: true,
  });
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
const render = async (props: Partial<React.ComponentProps<typeof DotsActorPanel>> = {}) => {
  await act(async () =>
    root.render(<DotsActorPanel groupId="g_a" actorId="dot" isVisible setup {...props} />),
  );
};
const button = (text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === text)!;
it("only reads on opening and keeps native task instructions in the accepted setup response", async () => {
  await render();
  expect(mocks.load).toHaveBeenCalledWith("g_a", "dot");
  expect(mocks.update).not.toHaveBeenCalled();
  expect(host.querySelector("a")?.href).toBe("https://chatgpt.com/dots/home");
  await act(async () => button("dotsActor.connect").click());
  expect(mocks.update).toHaveBeenCalledWith("g_a", "dot", "connect", undefined);
  expect(host.textContent).toContain("Create a native Work event task");
  await act(async () => button("dotsActor.copyInstructions").click());
  expect(mocks.copy).toHaveBeenCalledWith(expect.stringContaining("actor_token=synthetic-only"));
});
it("does not allow connection changes while CCCC access is enabled or unresolved work remains", async () => {
  mocks.load.mockResolvedValue(
    state({ enabled: true, connected: true, subscribed: true, phase: "ready" }),
  );
  await render();
  expect(button("dotsActor.disconnect").disabled).toBe(true);
  expect(host.textContent).toContain("dotsActor.pauseBeforeChanging");
  mocks.load.mockResolvedValue(
    state({ connected: true, subscribed: true, active_turn_id: "turn1", phase: "failed" }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(button("dotsActor.disconnect").disabled).toBe(true);
});
it("requires review confirmation and sends only the exact unfinished turn", async () => {
  mocks.load.mockResolvedValue(
    state({ connected: true, subscribed: true, phase: "failed", active_turn_id: "turn_exact" }),
  );
  await render();
  vi.mocked(confirm).mockReturnValue(false);
  await act(async () => button("dotsActor.resolve").click());
  expect(mocks.update).not.toHaveBeenCalled();
  vi.mocked(confirm).mockReturnValue(true);
  await act(async () => button("dotsActor.resolve").click());
  expect(mocks.update).toHaveBeenCalledWith("g_a", "dot", "resolve", "turn_exact");
});
it("publishes slow reads without overlapping and never treats callback acceptance as completion", async () => {
  let finish!: (value: ReturnType<typeof state>) => void;
  mocks.load.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render();
  await act(async () => vi.advanceTimersByTimeAsync(2_500));
  expect(mocks.load).toHaveBeenCalledTimes(1);
  await act(async () =>
    finish(
      state({
        enabled: true,
        connected: true,
        subscribed: true,
        phase: "delivered",
        pending_count: 2,
      }),
    ),
  );
  expect(host.textContent).toContain("dotsActor.phases.delivered");
  expect(host.textContent).not.toContain("dotsActor.phases.working");
  expect(host.textContent).not.toContain("dotsActor.lastExecution.done");
});
it("rejects late instructions after changing Group", async () => {
  let finishAction!: (value: unknown) => void;
  mocks.update.mockImplementation(
    () =>
      new Promise((resolve) => {
        finishAction = resolve;
      }),
  );
  await render();
  await act(async () => button("dotsActor.connect").click());
  mocks.load.mockResolvedValue(state({ group_id: "g_b" }));
  await render({ groupId: "g_b" });
  await act(async () =>
    finishAction({ ok: true, result: { instructions: "private-old-instruction" } }),
  );
  expect(host.textContent).not.toContain("private-old-instruction");
  expect(button("dotsActor.connect").disabled).toBe(false);
  expect(mocks.load).toHaveBeenLastCalledWith("g_b", "dot");
});
it("retires a hidden setup visit and does not display its late secret when reopened", async () => {
  let finish!: (value: unknown) => void;
  const busy = vi.fn();
  mocks.update.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render({ onBusyChange: busy });
  await act(async () => button("dotsActor.connect").click());
  await render({ isVisible: false, onBusyChange: busy });
  await render({ onBusyChange: busy });
  await act(async () => finish({ ok: true, result: { instructions: "old-hidden-secret" } }));
  expect(host.textContent).not.toContain("old-hidden-secret");
  expect(button("dotsActor.connect").disabled).toBe(false);
  expect(busy).toHaveBeenLastCalledWith(false);
});
it("keeps late read errors out of a new Actor visit", async () => {
  let finish!: (value: unknown) => void;
  mocks.load.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render();
  mocks.load.mockResolvedValue(state({ group_id: "g_b", phase: "ready", subscribed: true }));
  await render({ groupId: "g_b" });
  await act(async () => finish({ ok: false, error: { message: "old group read failed" } }));
  expect(host.textContent).toContain("dotsActor.phases.ready");
  expect(host.textContent).not.toContain("old group read failed");
});
it("localizes known delivery errors and shows only a concise completion summary", async () => {
  mocks.load.mockResolvedValue(
    state({
      phase: "failed",
      error_code: "dots_callback_rejected",
      last_execution: {
        turn_id: "private-turn",
        event_ids: ["private-event"],
        status: "done",
        completed_at: "2026-10-11T00:00:00Z",
      },
    }),
  );
  await render({ setup: false });
  expect(host.textContent).toContain("dotsActor.errors.callbackRejected");
  expect(host.textContent).toContain("dotsActor.lastExecution.done");
  expect(host.textContent).toContain("dotsActor.notifyAgain");
  expect(button("dotsActor.resolve")).toBeUndefined();
  expect(host.textContent).not.toContain("private-turn");
  expect(host.textContent).not.toContain("private-event");
  expect(mocks.update).not.toHaveBeenCalled();
});
it.each([
  ["en", "10/12/2026, 1:34:56 PM"],
  ["zh", "2026/10/12 13:34:56"],
  ["ja", "2026/10/12 13:34:56"],
])("shows a neutral completion timestamp and %s subscription expiry", async (language, expiry) => {
  mocks.language = language;
  const now = new Date(2026, 9, 12, 12, 34, 56);
  vi.setSystemTime(now);
  mocks.load.mockResolvedValue(
    state({
      connected: true,
      subscribed: true,
      phase: "ready",
      subscription_expires_at: new Date(2026, 9, 12, 13, 34, 56).getTime(),
      last_execution: {
        turn_id: "turn-fixture",
        event_ids: ["event-fixture"],
        status: "done",
        completed_at: new Date(now.getTime() - 1_000).toISOString(),
      },
    }),
  );
  await render();
  expect(host.textContent).toContain("dotsActor.lastExecution.done · 12:34");
  expect(host.textContent).not.toContain("just now");
  expect(host.textContent).toContain(`dotsActor.subscriptionExpires: ${expiry}`);
});
it("gives a localized setup action for missing shared configuration", async () => {
  mocks.update.mockResolvedValue({
    ok: false,
    error: {
      code: "connector_unavailable",
      message: "Configure the shared ChatGPT connector first",
    },
  });
  await render();
  await act(async () => button("dotsActor.connect").click());
  expect(host.textContent).toContain("dotsActor.errors.connectorRequired");
  expect(host.textContent).not.toContain("Configure the shared ChatGPT connector first");
});
it("keeps the last authoritative status through read failure and preserves action errors after read recovery", async () => {
  mocks.load.mockResolvedValue(state({ connected: true, subscribed: true, phase: "stopped" }));
  await render();
  mocks.update.mockResolvedValue({ ok: false, error: { message: "action rejected" } });
  await act(async () => button("dotsActor.disconnect").click());
  mocks.load.mockResolvedValue({ ok: false, error: { message: "status unavailable" } });
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(host.textContent).toContain("dotsActor.phases.stopped");
  expect(host.textContent).toContain("status unavailable");
  mocks.load.mockResolvedValue(state({ connected: true, subscribed: true, phase: "stopped" }));
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(host.textContent).not.toContain("status unavailable");
  expect(host.textContent).toContain("action rejected");
});

it("lets a newly enabled Actor connect directly from its runtime pane and explains native-task setup", async () => {
  mocks.load.mockResolvedValue(state({ enabled: true }));
  await render({ setup: false, isRunning: true });
  const connect = button("dotsActor.connect");
  expect(connect).toBeDefined();
  expect(connect.disabled).toBe(false);
  expect(mocks.update).not.toHaveBeenCalled();
  await act(async () => connect.click());
  expect(mocks.update).toHaveBeenCalledWith("g_a", "dot", "connect", undefined);
  expect(host.textContent).toContain("dotsActor.nativeStepCopy");
  expect(host.textContent).toContain("dotsActor.nativeStepPasteConfirm");
  expect(host.textContent).toContain("dotsActor.nativeStepObserve");
  await act(async () => button("dotsActor.copyInstructions").click());
  expect(mocks.copy).toHaveBeenCalledOnce();
});

it("can retrieve instructions for the same subscribed connection without pausing or disconnecting it", async () => {
  mocks.load.mockResolvedValue(
    state({ enabled: true, connected: true, subscribed: true, phase: "ready" }),
  );
  await render({ setup: false, isRunning: true });
  const instructions = button("dotsActor.getInstructions");
  expect(instructions).toBeDefined();
  expect(instructions.disabled).toBe(false);
  await act(async () => instructions.click());
  expect(mocks.update).toHaveBeenCalledWith("g_a", "dot", "connect", undefined);
  expect(mocks.update).toHaveBeenCalledOnce();
});

it("keeps connection-changing controls locked for an unfinished turn and exposes no writable setup to read-only users", async () => {
  mocks.load.mockResolvedValue(
    state({ connected: true, phase: "working", active_turn_id: "turn-active" }),
  );
  await render({ setup: false });
  expect(button("dotsActor.getInstructions").disabled).toBe(true);
  await render({ setup: false, readOnly: true });
  expect(button("dotsActor.getInstructions")).toBeUndefined();
  expect(button("dotsActor.disconnect")).toBeUndefined();
  expect(button("dotsActor.sharedSettings")).toBeUndefined();
  expect(mocks.update).not.toHaveBeenCalled();
});

it("explains an unconnected read-only Actor and keeps the expired subscription time visible in the runtime pane", async () => {
  vi.setSystemTime(new Date(2026, 9, 12, 14, 34, 56));
  mocks.load.mockResolvedValue(state());
  await render({ setup: false, readOnly: true });
  expect(host.textContent).toContain("dotsActor.adminSetup");
  mocks.load.mockResolvedValue(
    state({
      connected: true,
      subscribed: false,
      phase: "waiting_subscription",
      subscription_expires_at: new Date(2026, 9, 12, 13, 34, 56).getTime(),
    }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(host.textContent).toContain("dotsActor.subscriptionExpired:");
});

it("publishes only the current Actor's accepted GET snapshot and retains it on a later read failure", async () => {
  const onStateChange = vi.fn();
  mocks.load.mockResolvedValueOnce(state({ group_id: "g_other", phase: "ready" }));
  await render({ setup: false, onStateChange });
  expect(onStateChange).not.toHaveBeenCalled();
  mocks.load.mockResolvedValue(state({ connected: true, subscribed: true, phase: "ready" }));
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ phase: "ready" }));
  mocks.load.mockRejectedValue(new Error("synthetic read failure"));
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(onStateChange).toHaveBeenCalledOnce();
  expect(host.textContent).toContain("dotsActor.phases.ready");
  expect(host.textContent).toContain("dotsActor.readFailed");
});

it("does not label a reopened setup as copied when an earlier clipboard operation finishes late", async () => {
  let copied!: () => void;
  mocks.copy.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        copied = resolve;
      }),
  );
  await render();
  await act(async () => button("dotsActor.connect").click());
  await act(async () => button("dotsActor.copyInstructions").click());
  await render({ isVisible: false });
  await render();
  await act(async () => button("dotsActor.connect").click());
  await act(async () => copied());
  expect(button("dotsActor.copied")).toBeUndefined();
  expect(button("dotsActor.copyInstructions")).toBeDefined();
});

it("does not read or expose Dots connection controls when the current role cannot access the instance connection", async () => {
  await render({ canAccessConnection: false });
  expect(host.textContent).toContain("dotsActor.adminStatus");
  expect(host.textContent).toContain("dotsActor.adminManaged");
  expect(host.querySelector("a")?.href).toBe("https://chatgpt.com/dots/home");
  expect(host.querySelectorAll("button")).toHaveLength(0);
  await act(async () => vi.advanceTimersByTimeAsync(4_000));
  expect(mocks.load).not.toHaveBeenCalled();
});

it("retires private instructions and rejects a pending action across an administrator-role change", async () => {
  let finish!: (value: unknown) => void;
  mocks.update.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render();
  await act(async () => button("dotsActor.connect").click());
  await render({ canAccessConnection: false });
  await act(async () =>
    finish({ ok: true, result: { instructions: "synthetic retired private setup" } }),
  );
  expect(host.textContent).not.toContain("synthetic retired private setup");
  expect(host.querySelector("textarea")).toBeNull();
  const reads = mocks.load.mock.calls.length;
  await act(async () => vi.advanceTimersByTimeAsync(4_000));
  expect(mocks.load).toHaveBeenCalledTimes(reads);
  await render();
  expect(host.textContent).not.toContain("synthetic retired private setup");
  expect(button("dotsActor.connect").disabled).toBe(false);
});

it("shows a permanent subscription and the latest delivery steps once subscribed", async () => {
  mocks.load.mockResolvedValue(
    state({ connected: true, subscribed: true, phase: "delivered", subscription_expires_at: null }),
  );
  await render();
  expect(host.textContent).toContain("dotsActor.status.permanent");
  const steps = [...host.querySelectorAll("li[data-step]")].map(
    (step) => `${step.getAttribute("data-step")}:${step.getAttribute("data-state")}`,
  );
  // Accepted by ChatGPT is not claimed and not completed.
  expect(steps).toEqual(["queued:done", "woken:done", "claimed:current", "completed:todo"]);
  expect(host.textContent).not.toContain("dotsActor.setup.copyTitle");
  expect(button("dotsActor.getInstructions")).toBeTruthy();
});

it("guides an unsubscribed Actor through the numbered setup instead of delivery steps", async () => {
  mocks.load.mockResolvedValue(
    state({ connected: false, subscribed: false, phase: "unconfigured" }),
  );
  await render({ onOpenSharedSettings: vi.fn() });
  expect(host.textContent).toContain("dotsActor.status.notSubscribed");
  expect(host.querySelector("li[data-step]")).toBeNull();
  for (const key of ["pluginTitle", "copyTitle", "waitTitle", "tipsTitle"])
    expect(host.textContent).toContain(`dotsActor.setup.${key}`);
  expect(button("dotsActor.sharedSettings")).toBeTruthy();
});

it("skips the plugin step for an Actor connected before, such as after its subscription expired", async () => {
  mocks.load.mockResolvedValue(
    state({
      connected: true,
      subscribed: false,
      phase: "waiting_subscription",
      subscription_expires_at: 1,
    }),
  );
  await render({ onOpenSharedSettings: vi.fn() });
  const plugin = host.querySelector('li[data-setup-step="1"]')!;
  expect(plugin.hasAttribute("data-done")).toBe(true);
  expect(plugin.textContent).toContain("dotsActor.setup.pluginDone");
  expect(plugin.querySelector("button")).toBeNull();
  // Getting the instructions again is the one thing left to do.
  expect(button("dotsActor.getInstructions")).toBeTruthy();
});
