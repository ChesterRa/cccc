// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { DotsActorState } from "../../services/api/webModel";

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  update: vi.fn(),
  copy: vi.fn(),
  connectors: vi.fn(),
  create: vi.fn(),
  stop: vi.fn(),
  language: "en",
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { time?: string }) =>
      options?.time ? `${key}: ${options.time}` : key,
    i18n: { language: mocks.language },
  }),
}));
vi.mock("../../services/api", () => ({
  fetchWebModelConnectors: () => mocks.connectors(),
  createWebModelConnector: (...args: unknown[]) => mocks.create(...args),
  fetchDotsActorState: (...args: unknown[]) => mocks.load(...args),
  updateDotsActorConnection: (...args: unknown[]) => mocks.update(...args),
  stopActor: (...args: unknown[]) => mocks.stop(...args),
}));
import { DotsActorPanel } from "./DotsActorPanel";

const state = (patch: Partial<DotsActorState> = {}) => ({
  ok: true as const,
  result: {
    group_id: "g_a",
    actor_id: "dot",
    enabled: false,
    connected: false,
    paired: Boolean(patch.subscribed),
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
  mocks.stop.mockResolvedValue({ ok: true, result: {} });
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: {
      connectors: [{ connector_id: "shared", provider: "chatgpt_web" }],
      connector_base_url: "https://cccc.example.test",
    },
  });
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
    root.render(<DotsActorPanel groupId="g_a" actorId="dot" isVisible {...props} />),
  );
};
const button = (text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === text)!;
it("distinguishes a paired paused Dot from an incomplete pairing without enabling it", async () => {
  mocks.load.mockResolvedValue(
    state({ connected: true, paired: true, enabled: false, phase: "stopped" }),
  );
  await render({ setup: true });
  expect(host.textContent).toContain("dotsActor.pairedPaused");
  expect(host.textContent).toContain("dotsActor.status.paired");
  expect(mocks.update).not.toHaveBeenCalled();
  expect(mocks.stop).not.toHaveBeenCalled();
  mocks.load.mockResolvedValue(
    state({ connected: true, paired: true, enabled: true, phase: "waiting_subscription" }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(host.textContent).not.toContain("dotsActor.pairedPaused");
});

it("keeps Dot pairing setup visible when the native task subscribed before pairing", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      ...state({ connected: true, enabled: true, subscribed: true }).result,
      paired: false,
      phase: "waiting_pairing",
      pending_count: 2,
    },
  });
  await render();
  expect(host.textContent).toContain("dotsActor.phases.waiting_pairing");
  expect(host.textContent).toContain("dotsActor.setup.copyTitle");
  expect(host.textContent).not.toContain("nativeActor.deliveryTitle");
  expect(button("dotsActor.getInstructions").disabled).toBe(false);
  expect(mocks.update).not.toHaveBeenCalled();
});
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
it.each([
  [false, false, "waiting_pairing"],
  [false, true, "waiting_pairing"],
  [true, false, "waiting_subscription"],
] as const)(
  "keeps setup regeneration reachable after code expiry (paired=%s, subscribed=%s)",
  async (paired, subscribed, phase) => {
    mocks.load.mockResolvedValue(state({ connected: true, paired, subscribed, phase }));
    mocks.update.mockResolvedValueOnce({
      ok: true,
      result: { instructions: "synthetic old setup" },
    });
    await render();
    await act(async () => button("dotsActor.getInstructions").click());
    await act(async () => button("dotsActor.copyInstructions").click());
    expect(button("dotsActor.copied")).toBeDefined();
    await act(async () => vi.advanceTimersByTimeAsync(10 * 60_000 + 1));
    const regenerate = button("dotsActor.getInstructions");
    expect(regenerate).toBeDefined();
    expect(regenerate.disabled).toBe(false);
    expect(mocks.update).toHaveBeenCalledOnce();
    mocks.update.mockResolvedValueOnce({
      ok: true,
      result: { instructions: "synthetic fresh setup" },
    });
    await act(async () => regenerate.click());
    expect(host.querySelector("textarea")!.value).toBe("synthetic fresh setup");
    expect(button("dotsActor.copied")).toBeUndefined();
    expect(mocks.update).toHaveBeenCalledTimes(2);
    expect(mocks.update).toHaveBeenLastCalledWith("g_a", "dot", "connect", undefined);
    await act(async () => button("dotsActor.copyInstructions").click());
    expect(mocks.copy).toHaveBeenLastCalledWith("synthetic fresh setup");
  },
);
it("blocks copying the old setup during regeneration and leaves failure recovery available", async () => {
  mocks.load.mockResolvedValue(state({ connected: true, phase: "waiting_pairing" }));
  mocks.update.mockResolvedValueOnce({ ok: true, result: { instructions: "synthetic old setup" } });
  await render();
  await act(async () => button("dotsActor.getInstructions").click());
  const regenerate = button("dotsActor.getInstructions");
  expect(regenerate).toBeDefined();
  let finish!: (value: unknown) => void;
  mocks.update.mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
  await act(async () => regenerate.click());
  expect(button("dotsActor.getInstructions").disabled).toBe(true);
  expect(button("dotsActor.copyInstructions").disabled).toBe(true);
  await act(async () => button("dotsActor.getInstructions").click());
  expect(mocks.update).toHaveBeenCalledTimes(2);
  await act(async () =>
    finish({ ok: false, error: { message: "synthetic regeneration rejected" } }),
  );
  expect(host.querySelector('[role="alert"]')!.textContent).toBe("synthetic regeneration rejected");
  expect(host.querySelector("textarea")!.value).toBe("synthetic old setup");
  expect(button("dotsActor.getInstructions").disabled).toBe(false);
  mocks.update.mockResolvedValueOnce({
    ok: true,
    result: { instructions: "synthetic recovered setup" },
  });
  await act(async () => button("dotsActor.getInstructions").click());
  expect(host.querySelector("textarea")!.value).toBe("synthetic recovered setup");
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(button("dotsActor.copyInstructions").disabled).toBe(false);
});
it("keeps task-update guidance reachable for an enabled Actor waiting for its Dot to claim", async () => {
  mocks.load.mockResolvedValue(
    state({
      enabled: true,
      connected: true,
      subscribed: true,
      phase: "delivered",
      pending_count: 1,
    }),
  );
  await render({ setup: true });
  expect(mocks.update).not.toHaveBeenCalled();
  expect(host.textContent).not.toContain("dotsActor.nativeStepPasteConfirm");
  await act(async () => button("dotsActor.getInstructions").click());
  expect(mocks.update).toHaveBeenCalledTimes(1);
  expect(mocks.update).toHaveBeenCalledWith("g_a", "dot", "connect", undefined);
  expect(host.textContent).toContain("dotsActor.nativeStepPasteConfirm");
  expect(host.querySelector("dl")).toBeNull();
  expect(host.querySelector("li[data-step]")).toBeNull();
  expect(button("dotsActor.disconnect").disabled).toBe(false);
  await act(async () => button("dotsActor.copyInstructions").click());
  expect(mocks.copy).toHaveBeenCalledWith(host.querySelector("textarea")!.value);
});
it("offers a confirmed pause-and-disconnect while idle and still blocks unresolved work", async () => {
  mocks.load.mockResolvedValue(
    state({ enabled: true, connected: true, subscribed: true, phase: "ready" }),
  );
  await render({ setup: true });
  expect(button("dotsActor.disconnect").disabled).toBe(false);
  expect(host.textContent).toContain("dotsActor.pauseBeforeChanging");
  mocks.load.mockResolvedValue(
    state({ connected: true, subscribed: true, active_turn_id: "turn1", phase: "failed" }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(button("dotsActor.disconnect").disabled).toBe(true);
});

it("pauses only after disconnect confirmation and never disconnects if pausing fails", async () => {
  mocks.load.mockResolvedValue(
    state({ enabled: true, connected: true, subscribed: true, phase: "ready" }),
  );
  await render({ setup: true });
  vi.mocked(confirm).mockReturnValue(false);
  await act(async () => button("dotsActor.disconnect").click());
  expect(mocks.stop).not.toHaveBeenCalled();
  expect(mocks.update).not.toHaveBeenCalled();
  vi.mocked(confirm).mockReturnValue(true);
  mocks.stop.mockResolvedValue({ ok: false, error: { message: "pause failed" } });
  await act(async () => button("dotsActor.disconnect").click());
  expect(mocks.stop).toHaveBeenCalledExactlyOnceWith("g_a", "dot");
  expect(mocks.update).not.toHaveBeenCalled();
  expect(host.textContent).toContain("pause failed");
});

it("waits for pause before disconnecting the same Actor and preserves a newly discovered unfinished turn", async () => {
  mocks.load.mockResolvedValue(
    state({ enabled: true, connected: true, subscribed: true, phase: "ready" }),
  );
  await render({ setup: true, isRunning: true });
  let finish!: (value: unknown) => void;
  mocks.stop.mockReturnValue(new Promise((resolve) => (finish = resolve)));
  await act(async () => button("dotsActor.disconnect").click());
  expect(mocks.stop).toHaveBeenCalledOnce();
  expect(mocks.update).not.toHaveBeenCalled();
  mocks.load.mockResolvedValue(
    state({
      enabled: false,
      connected: true,
      subscribed: true,
      phase: "interrupted",
      active_turn_id: "just-claimed",
    }),
  );
  mocks.update.mockResolvedValue({
    ok: false,
    error: { code: "delivery_unresolved", message: "unfinished" },
  });
  await act(async () => finish({ ok: true, result: {} }));
  expect(mocks.update).toHaveBeenCalledExactlyOnceWith("g_a", "dot", "disconnect", undefined);
  expect(host.textContent).toContain("nativeActor.connectionBusy");
  expect(button("dotsActor.disconnect").disabled).toBe(true);
  expect(button("nativeActor.endTurn")).toBeDefined();
});
it("requires review confirmation and sends only the exact unfinished turn", async () => {
  mocks.load.mockResolvedValue(
    state({ connected: true, subscribed: true, phase: "failed", active_turn_id: "turn_exact" }),
  );
  await render();
  vi.mocked(confirm).mockReturnValue(false);
  await act(async () => button("nativeActor.endTurn").click());
  expect(mocks.update).not.toHaveBeenCalled();
  vi.mocked(confirm).mockReturnValue(true);
  await act(async () => button("nativeActor.endTurn").click());
  expect(mocks.update).toHaveBeenCalledWith("g_a", "dot", "resolve", "turn_exact");
});
it.each([
  { setup: false, compact: false },
  { setup: false, compact: true },
  { setup: true, compact: false },
])("can explicitly end an enabled claimed turn without pausing (%j)", async (props) => {
  mocks.load.mockResolvedValue(
    state({
      enabled: true,
      connected: true,
      subscribed: true,
      phase: "working",
      active_turn_id: "stuck-turn",
    }),
  );
  await render({ ...props, onExpand: vi.fn() });
  const end = button("nativeActor.endTurn");
  expect(end).toBeDefined();
  if (!props.setup)
    expect(
      host.querySelector('section[aria-label="nativeActor.deliveryTitle"]')?.contains(end),
    ).toBe(true);
  expect(mocks.update).not.toHaveBeenCalled();
  vi.mocked(confirm).mockReturnValue(false);
  await act(async () => end.click());
  expect(mocks.update).not.toHaveBeenCalled();
  vi.mocked(confirm).mockReturnValue(true);
  mocks.load.mockResolvedValue(
    state({ enabled: true, connected: true, subscribed: true, phase: "ready" }),
  );
  await act(async () => end.click());
  expect(mocks.update).toHaveBeenCalledExactlyOnceWith("g_a", "dot", "resolve", "stuck-turn");
  expect(mocks.stop).not.toHaveBeenCalled();
  expect(button("nativeActor.endTurn")).toBeUndefined();
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
  expect(button("nativeActor.endTurn")).toBeUndefined();
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
  await render({ setup: true });
  mocks.update.mockResolvedValue({ ok: false, error: { message: "action rejected" } });
  await act(async () => button("dotsActor.disconnect").click());
  mocks.load.mockResolvedValue({ ok: false, error: { message: "status unavailable" } });
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(button("dotsActor.disconnect").disabled).toBe(false);
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
  await render({ setup: true, isRunning: true });
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
  await render({ setup: true });
  expect(button("dotsActor.getInstructions").disabled).toBe(true);
  await render({ setup: true, readOnly: true });
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
  // Accepted by ChatGPT means waiting for the Dot, not working or completed.
  expect(steps).toEqual(["notify:done", "waiting:current", "working:todo", "completed:todo"]);
  const current = host.querySelector('li[aria-current="step"]')!;
  expect(current.getAttribute("data-step")).toBe("waiting");
  expect(current.querySelector("svg")?.getAttribute("class")).toContain("clock");
  expect(current.querySelector(".animate-spin")).toBeNull();
  expect(host.textContent).not.toContain("dotsActor.steps.workingHint");
  expect(host.textContent).not.toContain("dotsActor.setup.copyTitle");
  expect(button("dotsActor.getInstructions")).toBeUndefined();
  await render({ setup: true });
  expect(button("dotsActor.getInstructions")).toBeTruthy();
  expect(host.querySelector("li[data-step]")).toBeNull();
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

it("keeps shared plugin setup reachable after a local Connect without a subscription", async () => {
  const onOpenSharedSettings = vi.fn();
  await render({ onOpenSharedSettings });
  mocks.load.mockResolvedValue(
    state({ connected: true, subscribed: false, phase: "waiting_subscription" }),
  );
  await act(async () => button("dotsActor.connect").click());
  const plugin = host.querySelector('li[data-setup-step="1"]')!;
  expect(plugin.hasAttribute("data-done")).toBe(false);
  expect(plugin.textContent).not.toContain("dotsActor.setup.pluginDone");
  expect(button("dotsActor.sharedSettings")).toBeDefined();
  await act(async () => button("dotsActor.sharedSettings").click());
  expect(onOpenSharedSettings).toHaveBeenCalledOnce();
  expect(mocks.update).toHaveBeenCalledOnce();
});

it.each([
  ["working", false],
  ["interrupted", false],
  ["working", true],
] as const)(
  "keeps an accepted %s turn visible after subscription expiry (readOnly=%s)",
  async (phase, readOnly) => {
    mocks.load.mockResolvedValue(
      state({
        enabled: true,
        connected: true,
        subscribed: false,
        phase,
        active_turn_id: "accepted-turn",
        subscription_expires_at: 1,
      }),
    );
    await render({ readOnly });
    expect(host.querySelector('li[data-step="working"]')).not.toBeNull();
    expect(host.querySelector("li[data-setup-step]")).toBeNull();
    expect(host.textContent).toContain(`dotsActor.phases.${phase}`);
    expect(host.textContent).toContain("dotsActor.subscriptionExpired");
    expect(host.textContent).not.toContain("dotsActor.adminSetup");
    if (!readOnly) expect(button("nativeActor.endTurn").disabled).toBe(false);
    else expect(button("nativeActor.endTurn")).toBeUndefined();
    if (!readOnly) {
      expect(button("dotsActor.getInstructions")).toBeUndefined();
      await render({ setup: true });
      expect(button("dotsActor.getInstructions").disabled).toBe(true);
      expect(host.querySelector("li[data-step]")).toBeNull();
    }
    expect(mocks.update).not.toHaveBeenCalled();
  },
);

it("shows the current stage and spins only while sending notifications", async () => {
  const cases = [
    ["pending", "notify", false],
    ["delivering", "notify", true],
    ["delivered", "waiting", false],
    ["working", "working", false],
    ["interrupted", "working", false],
    ["failed", "notify", false],
  ] as const;
  for (const [index, [phase, stage, spinning]] of cases.entries()) {
    mocks.load.mockResolvedValue(
      state({ enabled: true, connected: true, subscribed: true, phase }),
    );
    if (index === 0) await render();
    else await act(async () => vi.advanceTimersByTimeAsync(2_000));
    const current = host.querySelector('li[aria-current="step"]')!;
    expect(current.getAttribute("data-step")).toBe(stage);
    expect(Boolean(current.querySelector(".animate-spin"))).toBe(spinning);
    if (spinning)
      expect(current.querySelector(".animate-spin")?.getAttribute("class")).toContain(
        "motion-reduce:animate-none",
      );
    else if (phase !== "interrupted" && phase !== "failed")
      expect(current.querySelector("svg")?.getAttribute("class")).toContain("clock");
    if (phase === "delivering") expect(current.textContent).toContain("nativeActor.steps.sending");
    if (phase === "failed") {
      expect(current.textContent).toContain("nativeActor.failed");
      expect(current.textContent).not.toContain("dotsActor.steps.notifyHint");
    }
    if (phase === "interrupted") {
      expect(current.textContent).toContain("nativeActor.interrupted");
      expect(current.textContent).not.toContain("dotsActor.steps.workingHint");
    }
  }
  expect(mocks.update).not.toHaveBeenCalled();
});

it.each(["partial", "cancelled", "failed"] as const)(
  "keeps a previous %s receipt in the summary rather than showing an active failure",
  async (status) => {
    mocks.load.mockResolvedValue(
      state({
        connected: true,
        subscribed: true,
        phase: "waiting_subscription",
        last_execution: {
          turn_id: "historical-turn",
          event_ids: ["historical-source"],
          status,
          completed_at: "2026-10-12T00:00:00Z",
        },
      }),
    );
    await render();
    expect(host.textContent).toContain(`dotsActor.lastExecution.${status}`);
    expect(host.querySelector("li[data-step]")).toBeNull();
    expect(host.querySelector('[aria-current="step"]')).toBeNull();
    expect(host.querySelector(".animate-spin")).toBeNull();
    expect(host.querySelector('section[aria-label="nativeActor.deliveryTitle"]')).toBeNull();
    expect(mocks.update).not.toHaveBeenCalled();
  },
);

it.each(["ready", "stopped"] as const)(
  "labels a previous completed turn as history while the Actor is %s",
  async (phase) => {
    const now = new Date(2026, 9, 13, 12, 34, 56);
    vi.setSystemTime(now);
    mocks.load.mockResolvedValue(
      state({
        connected: true,
        subscribed: true,
        phase,
        last_execution: {
          turn_id: "historical-turn",
          event_ids: ["historical-source"],
          status: "done",
          completed_at: new Date(now.getTime() - 1_000).toISOString(),
        },
      }),
    );
    await render();
    const summary = host.querySelector("dl")!;
    expect(summary.textContent).toContain("dotsActor.lastExecution.done");
    expect(summary.textContent).toContain("12:34");
    expect(host.querySelector('section[aria-label="nativeActor.deliveryTitle"]')).toBeNull();
    expect(host.querySelector("li[data-step]")).toBeNull();
    expect(host.querySelector('[aria-current="step"]')).toBeNull();
    expect(host.querySelector(".animate-spin")).toBeNull();
    expect(host.textContent).toContain(`dotsActor.phases.${phase}`);
    expect(mocks.update).not.toHaveBeenCalled();
  },
);

it("shows unknown summary facts on initial read failure, then retains the last accepted snapshot", async () => {
  mocks.load.mockResolvedValue({ ok: false, error: { message: "status unavailable" } });
  await render();
  const summary = () => [...host.querySelectorAll("dl dd")].map((cell) => cell.textContent);
  expect(summary()).toEqual(Array(3).fill("dotsActor.unavailable"));
  expect(button("dotsActor.connect").disabled).toBe(true);

  mocks.load.mockResolvedValue(
    state({
      connected: true,
      subscribed: true,
      phase: "ready",
      pending_count: 3,
      last_execution: {
        turn_id: "completed-turn",
        event_ids: ["completed-source"],
        status: "done",
        completed_at: "2026-10-12T00:00:00Z",
      },
    }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  const accepted = summary();
  expect(accepted[0]).toBe("dotsActor.status.permanent");
  expect(accepted[2]).toContain("dotsActor.lastExecution.done");

  mocks.load.mockResolvedValue({ ok: false, error: { message: "later read failed" } });
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(summary()).toEqual(accepted);
  expect(host.textContent).toContain("later read failed");
  expect(mocks.update).not.toHaveBeenCalled();
});

it("does not ask an administrator to connect a read-only Actor whose first status read failed", async () => {
  mocks.load.mockResolvedValue({ ok: false, error: { message: "status unavailable" } });
  await render({ readOnly: true });
  expect([...host.querySelectorAll("dl dd")].map((cell) => cell.textContent)).toEqual(
    Array(3).fill("dotsActor.unavailable"),
  );
  expect(host.textContent).not.toContain("dotsActor.adminSetup");
  expect(host.querySelectorAll("button")).toHaveLength(0);

  mocks.load.mockResolvedValue(state());
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(host.textContent).toContain("dotsActor.adminSetup");
  expect(mocks.update).not.toHaveBeenCalled();
});

it("creates the missing shared connector from step 1 and hands over its private URL only by copy", async () => {
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: { connectors: [], connector_base_url: "https://cccc.example.test" },
  });
  mocks.create.mockResolvedValue({
    ok: true,
    result: {
      connector: {
        connector_id: "shared",
        connector_url_path_token: "https://cccc.example.test/mcp/web-model/shared/token/test-only",
      },
    },
  });
  await render({ onOpenSharedSettings: vi.fn() });
  const step = host.querySelector("[data-dots-plugin-step]")!;
  expect(step.textContent).toContain("dotsActor.setup.noConnector");
  // The shared settings stay reachable beside the one-click creation.
  expect(button("dotsActor.sharedSettings")).toBeTruthy();
  await act(async () => button("dotsActor.setup.createConnector").click());
  expect(mocks.create).toHaveBeenCalledExactlyOnceWith("chatgpt_web", { createOnly: true });
  expect(step.querySelector("[data-chatgpt-plugin-setup]")).not.toBeNull();
  expect(host.textContent).not.toContain("test-only");
  expect(button("dotsActor.setup.createConnector")).toBeUndefined();
  await act(async () => button("webModelShared.copy").click());
  expect(mocks.copy).toHaveBeenLastCalledWith(
    "https://cccc.example.test/mcp/web-model/shared/token/test-only",
  );
});

it("offers no creation when a connector exists or the Web address is not public HTTPS", async () => {
  await render({ onOpenSharedSettings: vi.fn() });
  expect(button("dotsActor.setup.createConnector")).toBeUndefined();
  expect(mocks.create).not.toHaveBeenCalled();
  await act(async () => root.unmount());
  root = createRoot(host);
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: { connectors: [], connector_base_url: "http://localhost:8848" },
  });
  await render({ onOpenSharedSettings: vi.fn() });
  expect(button("dotsActor.setup.createConnector")).toBeUndefined();
  expect(host.textContent).toContain("webModelShared.publicUrlInvalid");
});

it("leaves a connector created elsewhere untouched when the page was stale", async () => {
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: { connectors: [], connector_base_url: "https://cccc.example.test" },
  });
  mocks.create.mockResolvedValue({
    ok: false,
    error: { code: "connector_exists", message: "exists" },
  });
  await render({ onOpenSharedSettings: vi.fn() });
  await act(async () => button("dotsActor.setup.createConnector").click());
  expect(host.textContent).toContain("dotsActor.setup.connectorExists");
  expect(host.querySelector("[data-chatgpt-plugin-setup]")).toBeNull();
  expect(button("dotsActor.setup.createConnector")).toBeUndefined();
  expect(button("dotsActor.sharedSettings")).toBeTruthy();
});

it("keeps a host dialog open while the one-time connector URL is being created", async () => {
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: { connectors: [], connector_base_url: "https://cccc.example.test" },
  });
  let finish!: (value: unknown) => void;
  mocks.create.mockReturnValue(new Promise((resolve) => (finish = resolve)));
  const onBusyChange = vi.fn();
  await render({ onOpenSharedSettings: vi.fn(), onBusyChange });
  onBusyChange.mockClear();
  await act(async () => button("dotsActor.setup.createConnector").click());
  expect(onBusyChange).toHaveBeenLastCalledWith(true);
  await act(async () =>
    finish({
      ok: true,
      result: {
        connector: {
          connector_id: "shared",
          connector_url_path_token:
            "https://cccc.example.test/mcp/web-model/shared/token/test-only",
        },
      },
    }),
  );
  expect(onBusyChange).toHaveBeenLastCalledWith(false);
  expect(host.querySelector("[data-chatgpt-plugin-setup]")).not.toBeNull();
});

it("blocks setup navigation and binding actions until the one-time connector response arrives", async () => {
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: { connectors: [], connector_base_url: "https://cccc.example.test" },
  });
  let finish!: (value: unknown) => void;
  mocks.create.mockReturnValue(new Promise((resolve) => (finish = resolve)));
  const openShared = vi.fn();
  await render({ onOpenSharedSettings: openShared });
  await act(async () => button("dotsActor.setup.createConnector").click());
  expect(button("dotsActor.sharedSettings").disabled).toBe(true);
  expect(button("dotsActor.connect").disabled).toBe(true);
  await act(async () => button("dotsActor.sharedSettings").click());
  expect(openShared).not.toHaveBeenCalled();
  await act(async () =>
    finish({
      ok: true,
      result: {
        connector: {
          connector_id: "shared",
          connector_url_path_token:
            "https://cccc.example.test/mcp/web-model/shared/token/probe-only",
        },
      },
    }),
  );
  expect(button("dotsActor.sharedSettings").disabled).toBe(false);
  expect(button("dotsActor.connect").disabled).toBe(false);
});

it.each(["network", "api"])(
  "reports a %s connector-read failure and allows an explicit retry",
  async (failure) => {
    mocks.connectors.mockResolvedValue({
      ok: true,
      result: { connectors: [], connector_base_url: "https://cccc.example.test" },
    });
    if (failure === "network") mocks.connectors.mockRejectedValueOnce(new Error("offline"));
    else
      mocks.connectors.mockResolvedValueOnce({
        ok: false,
        error: { code: "io_error", message: "probe-only failure" },
      });
    await render({ onOpenSharedSettings: vi.fn() });
    const step = host.querySelector("[data-dots-plugin-step]")!;
    expect(step.querySelector('[role="alert"]')?.textContent).toContain(
      "dotsActor.setup.connectorReadFailed",
    );
    expect(button("dotsActor.setup.createConnector")).toBeUndefined();
    expect(mocks.create).not.toHaveBeenCalled();
    await act(async () => button("common:retry").click());
    expect(mocks.connectors).toHaveBeenCalledTimes(2);
    expect(step.querySelector('[role="alert"]')).toBeNull();
    expect(button("dotsActor.setup.createConnector")).toBeTruthy();
    expect(mocks.create).not.toHaveBeenCalled();
  },
);

it("opens full details from a compact completed task without mutating its connection", async () => {
  mocks.load.mockResolvedValue(
    state({
      connected: true,
      paired: true,
      subscribed: true,
      phase: "ready",
      last_execution: {
        turn_id: "finished",
        event_ids: ["event"],
        status: "done",
        completed_at: "2026-10-19T00:00:00Z",
      },
    }),
  );
  const onExpand = vi.fn();
  await render({ compact: true, onExpand });
  expect(host.querySelectorAll("[data-step]")).toHaveLength(0);
  expect(host.textContent).toContain("dotsActor.lastExecution.done");
  expect(host.textContent).not.toContain("dotsActor.settings.instructionsHint");
  await act(async () => button("dotsActor.openDetails").click());
  expect(onExpand).toHaveBeenCalledOnce();
  expect(mocks.update).not.toHaveBeenCalled();
});

it("keeps a configured daily pane focused on status and opens the existing connection editor without writes", async () => {
  mocks.load.mockResolvedValue(state({ connected: true, subscribed: true, phase: "ready" }));
  const onSettings = vi.fn();
  await render({ setup: false, onSettings });
  expect(host.querySelector('section[aria-label="dotsActor.connectionSettings"]')).toBeNull();
  expect(button("dotsActor.getInstructions")).toBeUndefined();
  expect(host.querySelector("li[data-step]")).toBeNull();
  expect(button("dotsActor.connectionSettings")).toBeUndefined();
  expect(onSettings).not.toHaveBeenCalled();
  expect(mocks.update).not.toHaveBeenCalled();
  await render({ setup: true, onSettings });
  expect(button("dotsActor.getInstructions")).toBeDefined();
  expect(button("dotsActor.connectionSettings")).toBeUndefined();
});

it.each(["ready", "delivered", "working", "interrupted"] as const)(
  "keeps %s task overview out of connection editing while retaining task-dependent controls",
  async (phase) => {
    const unfinished = phase === "working" || phase === "interrupted";
    mocks.load.mockResolvedValue(
      state({
        connected: true,
        subscribed: true,
        phase,
        pending_count: 19,
        active_turn_id: unfinished ? "accepted-turn" : undefined,
        last_execution: {
          turn_id: "past",
          event_ids: ["past"],
          status: "done",
          completed_at: "2026-10-22T00:00:00Z",
        },
      }),
    );
    await render({ setup: true });
    expect(host.querySelector("dl")).toBeNull();
    expect(host.querySelector("li[data-step]")).toBeNull();
    expect(host.textContent).not.toContain("dotsActor.lastExecution.done");
    expect(host.textContent).not.toContain(`dotsActor.phases.${phase}`);
    expect(button("dotsActor.getInstructions").disabled).toBe(unfinished);
    if (phase === "working") expect(host.textContent).toContain("nativeActor.connectionBusy");
    if (unfinished) expect(button("nativeActor.endTurn")).toBeDefined();
    expect(mocks.update).not.toHaveBeenCalled();
  },
);

it("retains setup guidance and read-only restrictions in the daily pane", async () => {
  const onSettings = vi.fn();
  await render({ setup: false, onSettings });
  expect(button("dotsActor.connect")).toBeDefined();
  expect(host.querySelector("li[data-setup-step]")).not.toBeNull();
  await render({ setup: false, readOnly: true, onSettings });
  expect(button("dotsActor.connectionSettings")).toBeUndefined();
  expect(button("dotsActor.connect")).toBeUndefined();
  expect(host.textContent).toContain("dotsActor.adminSetup");
  expect(onSettings).not.toHaveBeenCalled();
  expect(mocks.update).not.toHaveBeenCalled();
});

it("keeps pairing and subscription feedback live in the connection editor without a delivery dashboard", async () => {
  mocks.load.mockResolvedValue(
    state({ connected: true, paired: false, subscribed: true, phase: "waiting_pairing" }),
  );
  await render({ setup: true });
  expect(host.textContent).toContain("dotsActor.status.notPaired");
  expect(host.textContent).toContain("dotsActor.status.permanent");
  mocks.load.mockResolvedValue(
    state({ connected: true, paired: true, subscribed: false, phase: "waiting_subscription" }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(host.textContent).toContain("dotsActor.status.paired");
  expect(host.textContent).toContain("dotsActor.status.notSubscribed");
  mocks.load.mockResolvedValue(
    state({ connected: true, paired: true, subscribed: true, phase: "ready" }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(host.textContent).toContain("dotsActor.status.paired");
  expect(host.textContent).toContain("dotsActor.status.permanent");
  expect(host.querySelector("dl,li[data-step]")).toBeNull();
  expect(mocks.update).not.toHaveBeenCalled();
});
it("keeps compact setup and interrupted task recovery reachable without automatic writes", async () => {
  const onExpand = vi.fn();
  await render({ compact: true, onExpand });
  await act(async () => button("dotsActor.openDetails").click());
  expect(onExpand).toHaveBeenCalledOnce();
  expect(mocks.update).not.toHaveBeenCalled();
  mocks.load.mockResolvedValue(
    state({
      connected: true,
      paired: true,
      subscribed: true,
      phase: "interrupted",
      active_turn_id: "unfinished",
      error_code: "dots_execution_interrupted",
    }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(host.textContent).toContain("dotsActor.errors.executionInterrupted");
  expect(button("nativeActor.endTurn")).not.toBeUndefined();
  await act(async () => button("nativeActor.endTurn").click());
  expect(mocks.update).toHaveBeenCalledWith("g_a", "dot", "resolve", "unfinished");
});
