// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
const mocks = vi.hoisted(() => ({ load: vi.fn(), change: vi.fn(), stop: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
  fetchGrokActorState: (...a: unknown[]) => mocks.load(...a),
  changeGrokWebhook: (...a: unknown[]) => mocks.change(...a),
  stopActor: (...a: unknown[]) => mocks.stop(...a),
}));
import { GrokActorPanel } from "./GrokActorPanel";
import type { GrokActorState } from "../../services/api";
const state: GrokActorState = {
  group_id: "g",
  actor_id: "bot",
  connected: true,
  bound: true,
  enabled: false,
  configured: false,
  phase: "unconfigured",
  pending_count: 0,
};
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
const busy = vi.fn();
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mocks.load.mockResolvedValue({ ok: true, result: state });
  mocks.change.mockResolvedValue({ ok: true, result: {} });
  mocks.stop.mockResolvedValue({ ok: true });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});
const button = (name: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === name)!;
async function render(props: Partial<Parameters<typeof GrokActorPanel>[0]> = {}) {
  await act(async () =>
    root.render(
      <GrokActorPanel groupId="g" actorId="bot" isVisible setup onBusyChange={busy} {...props} />,
    ),
  );
}
async function tick() {
  await act(async () => vi.advanceTimersByTime(2000));
}
async function fill(index: number, value: string) {
  await act(async () => {
    const input = host.querySelectorAll("input")[index];
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
it("saves masked credentials through one native setup path", async () => {
  await render();
  expect(host.querySelector("select")).toBeNull();
  expect(mocks.change).not.toHaveBeenCalled();
  expect(button("grokWebhook.save").disabled).toBe(true);
  await fill(0, "https://api2.cursor.sh/automations/webhook/synthetic");
  await fill(1, "synthetic-key");
  expect([...host.querySelectorAll("input")].every((i) => i.type === "password")).toBe(true);
  expect(host.textContent).not.toContain("synthetic-key");
  await act(async () => button("grokWebhook.save").click());
  expect(mocks.change).toHaveBeenCalledExactlyOnceWith("g", "bot", {
    action: "configure",
    url: "https://api2.cursor.sh/automations/webhook/synthetic",
    key: "synthetic-key",
  });
  expect(host.querySelectorAll("input")[1].value).toBe("");
  expect(busy).toHaveBeenLastCalledWith(false);
});
it("retains credentials after a failed save", async () => {
  await render();
  await fill(0, "https://api2.cursor.sh/automations/webhook/synthetic");
  await fill(1, "synthetic-key");
  mocks.change.mockRejectedValueOnce(new Error("offline"));
  await act(async () => button("grokWebhook.save").click());
  expect(host.querySelector("[role=alert]")?.textContent).toBe("offline");
  expect(host.querySelectorAll("input")[1].value).toBe("synthetic-key");
  expect(button("grokWebhook.save").disabled).toBe(false);
});
it("keeps Routine drafts for the same Bot and clears them when the saved Bot changes", async () => {
  const botA = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
  const botB = "https://grok.com/bot/958f2446-013f-4225-8dd3-f146295992d7";
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, enabled: true, bot_url: botA } });
  mocks.change.mockResolvedValue({ ok: true, result: { instructions: "Setup for fixture Bot A" } });
  await render();
  await fill(0, "https://api2.cursor.sh/automations/webhook/fixture-a");
  await fill(1, "fixture-a-key");
  await act(async () => button("grokWebhook.instructions").click());
  await tick();
  expect(host.querySelectorAll("input")[1].value).toBe("fixture-a-key");
  expect(host.querySelector("textarea")!.value).toBe("Setup for fixture Bot A");
  mocks.load.mockRejectedValueOnce(new Error("offline"));
  await tick();
  expect(host.querySelectorAll("input")[1].value).toBe("fixture-a-key");
  await render({ targetDirty: true });
  await tick();
  expect(host.querySelectorAll("input")[1].value).toBe("fixture-a-key");
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, enabled: true, bot_url: botB } });
  await tick();
  await render();
  expect([...host.querySelectorAll("input")].map((input) => input.value)).toEqual(["", ""]);
  expect(host.querySelector("textarea")).toBeNull();
  expect(button("grokWebhook.instructions").disabled).toBe(false);
  expect(button("grokWebhook.saveAndApply").disabled).toBe(true);
  expect(mocks.change.mock.calls.map((call) => call[2].action)).toEqual(["instructions"]);
  await fill(0, "https://api2.cursor.sh/automations/webhook/fixture-b");
  await fill(1, "fixture-b-key");
  await act(async () => button("grokWebhook.saveAndApply").click());
  expect(mocks.change).toHaveBeenLastCalledWith("g", "bot", {
    action: "configure",
    url: "https://api2.cursor.sh/automations/webhook/fixture-b",
    key: "fixture-b-key",
  });
});
it("requires a saved Bot but allows editing and applying while enabled and idle", async () => {
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, bound: false } });
  await render();
  expect(button("grokWebhook.instructions").disabled).toBe(true);
  expect(host.querySelector("input")!.disabled).toBe(true);
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, enabled: true } });
  await tick();
  expect(host.querySelector("input")!.disabled).toBe(false);
  expect(button("actors:dotsPause")).toBeUndefined();
  await fill(0, "https://api2.cursor.sh/automations/webhook/replacement");
  await fill(1, "synthetic-key");
  await act(async () => button("grokWebhook.saveAndApply").click());
  expect(mocks.change).toHaveBeenCalledExactlyOnceWith("g", "bot", {
    action: "configure",
    url: "https://api2.cursor.sh/automations/webhook/replacement",
    key: "synthetic-key",
  });
  expect(mocks.stop).not.toHaveBeenCalled();
});
it("keeps an editable draft during a task and enables apply only after completion", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, enabled: true, phase: "working", active_turn_id: "task" },
  });
  await render();
  await fill(0, "https://api2.cursor.sh/automations/webhook/replacement");
  await fill(1, "synthetic-key");
  expect(button("grokWebhook.saveAndApply").disabled).toBe(true);
  expect(host.textContent).toContain("grokWebhook.waitForTask");
  await tick();
  expect(host.querySelectorAll("input")[1].value).toBe("synthetic-key");
  expect(mocks.change).not.toHaveBeenCalled();
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, enabled: true } });
  await tick();
  expect(button("grokWebhook.saveAndApply").disabled).toBe(false);
  expect(host.querySelectorAll("input")[1].value).toBe("synthetic-key");
  await act(async () => button("grokWebhook.saveAndApply").click());
  expect(mocks.change).toHaveBeenCalledOnce();
  expect(mocks.stop).not.toHaveBeenCalled();
});
it("retains drafts and refreshes task state if a claim wins the apply race", async () => {
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, enabled: true } });
  await render();
  await fill(0, "https://api2.cursor.sh/automations/webhook/replacement");
  await fill(1, "synthetic-key");
  mocks.change.mockResolvedValueOnce({
    ok: false,
    error: { code: "delivery_unresolved", message: "unfinished task" },
  });
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, enabled: true, phase: "working", active_turn_id: "task" },
  });
  await act(async () => button("grokWebhook.saveAndApply").click());
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("grokWebhook.waitForTask");
  expect(host.querySelectorAll("input")[1].value).toBe("synthetic-key");
  expect(button("grokWebhook.saveAndApply").disabled).toBe(true);
  expect(mocks.stop).not.toHaveBeenCalled();
});
it("regenerates credential-free instructions without starting work", async () => {
  mocks.change.mockResolvedValue({ ok: true, result: { instructions: "handler" } });
  await render();
  await act(async () => button("grokWebhook.instructions").click());
  expect(host.querySelector("textarea")!.value).toBe("handler");
  await act(async () => button("grokWebhook.instructions").click());
  expect(mocks.change.mock.calls.map((call) => call[2])).toEqual([
    { action: "instructions" },
    { action: "instructions" },
  ]);
  expect(mocks.stop).not.toHaveBeenCalled();
});
it("rejects stale polls initiated before a successful action", async () => {
  await render();
  let finish!: (v: unknown) => void;
  mocks.load.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  await tick();
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, configured: true, phase: "stopped" },
  });
  await act(async () => button("grokWebhook.instructions").click());
  await act(async () => finish({ ok: true, result: state }));
  expect(host.textContent).toContain("grokWebhook.saved");
  expect(host.textContent).not.toContain("grokWebhook.phase.unconfigured");
});
it("clears pending UI state when an action finishes while hidden", async () => {
  await render();
  let finish!: (v: unknown) => void;
  mocks.change.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  await act(async () => button("grokWebhook.instructions").click());
  await render({ isVisible: false });
  await act(async () => finish({ ok: true, result: { instructions: "hidden" } }));
  await render();
  expect(button("grokWebhook.instructions").disabled).toBe(false);
  expect(busy).toHaveBeenLastCalledWith(false);
});
it("does not apply an old Actor's action result to a new Actor", async () => {
  await render();
  let finish!: (v: unknown) => void;
  mocks.change.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  await act(async () => button("grokWebhook.instructions").click());
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, actor_id: "other" } });
  await render({ actorId: "other" });
  await act(async () => finish({ ok: true, result: { instructions: "old handler" } }));
  expect(host.querySelector("textarea")).toBeNull();
});
it.each([
  { setup: true, enabled: false },
  { setup: false, enabled: false },
  { setup: true, enabled: true },
  { setup: false, enabled: true },
])(
  "retains exact unfinished task IDs across confirmation and polling (%j)",
  async ({ setup, enabled }) => {
    mocks.load.mockResolvedValue({
      ok: true,
      result: {
        ...state,
        enabled,
        phase: enabled ? "working" : "interrupted",
        active_turn_id: "older",
        active_event_ids: ["old"],
      },
    });
    await render({ setup });
    expect(mocks.change).not.toHaveBeenCalled();
    if (!setup)
      expect(
        host
          .querySelector('section[aria-label="nativeActor.deliveryTitle"]')
          ?.contains(button("nativeActor.endTurn")),
      ).toBe(true);
    await act(async () => button("nativeActor.endTurn").click());
    expect(mocks.change).not.toHaveBeenCalled();
    expect(host.textContent).toContain("nativeActor.endTurnConfirm");
    mocks.load.mockResolvedValue({
      ok: true,
      result: {
        ...state,
        enabled,
        phase: enabled ? "working" : "interrupted",
        active_turn_id: "newer",
        active_event_ids: ["new"],
      },
    });
    await tick();
    await act(async () => button("nativeActor.confirmEndTurn").click());
    expect(mocks.change).toHaveBeenCalledExactlyOnceWith("g", "bot", {
      action: "resolve",
      turn_id: "older",
      event_ids: ["old"],
    });
    expect(mocks.stop).not.toHaveBeenCalled();
  },
);
it("keeps explicit turn resolution reachable after connector revocation without requiring pause", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      ...state,
      connected: false,
      enabled: true,
      phase: "interrupted",
      active_turn_id: "task",
      active_event_ids: ["input"],
    },
  });
  await render();
  await act(async () => button("nativeActor.endTurn").click());
  await act(async () => button("nativeActor.confirmEndTurn").click());
  expect(mocks.change).toHaveBeenCalledExactlyOnceWith("g", "bot", {
    action: "resolve",
    turn_id: "task",
    event_ids: ["input"],
  });
  expect(mocks.stop).not.toHaveBeenCalled();
});
it("shows wake acceptance separately from confirmed completion", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, configured: true, phase: "delivered" },
  });
  await render({ setup: false });
  expect(host.querySelector('[aria-current="step"]')?.textContent).toContain(
    "grokActor.steps.waiting",
  );
  expect(host.textContent).toContain("grokActor.noTask");
});
it("does not read admin endpoints without connection access", async () => {
  await render({ canAccessConnection: false });
  await tick();
  expect(mocks.load).not.toHaveBeenCalled();
  expect(host.textContent).toContain("grokActor.adminManaged");
  expect(host.querySelector("button")).toBeNull();
});
it("has no mutations in a read-only view", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, phase: "failed", active_turn_id: "task" },
  });
  await render({ readOnly: true });
  expect(host.querySelector("button")).toBeNull();
  expect(mocks.change).not.toHaveBeenCalled();
});

it("does not present an earlier wake failure as a failed task after claim", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      ...state,
      configured: true,
      phase: "working",
      error_code: "grok_webhook_rejected",
      http_status: 503,
    },
  });
  await render();
  expect(host.querySelector('[role="alert"]')).toBeNull();
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      ...state,
      configured: true,
      phase: "failed",
      error_code: "grok_webhook_rejected",
      http_status: 503,
    },
  });
  await tick();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("grokWebhook.deliveryError");
});

it("keeps a completed receipt in the summary instead of showing a current completed workflow", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      ...state,
      configured: true,
      enabled: true,
      phase: "ready",
      last_execution: { status: "done", completed_at: "2026-10-22T00:00:00Z" },
    },
  });
  const onSettings = vi.fn();
  await render({ setup: false, onSettings });
  expect(host.querySelector("dl")?.textContent).toContain("grokActor.outcomes.done");
  expect(host.querySelector("li[data-step]")).toBeNull();
  expect(host.querySelector("input")).toBeNull();
  expect(button("grokActor.connectionSettings")).toBeUndefined();
  await render({ setup: false, compact: true, onSettings });
  await act(async () => button("grokActor.connectionSettings").click());
  expect(onSettings).toHaveBeenCalledOnce();
  expect(mocks.change).not.toHaveBeenCalled();
});

it.each([
  ["delivered", "waiting", "current"],
  ["unclaimed", "waiting", "error"],
  ["working", "working", "current"],
  ["interrupted", "working", "error"],
  ["failed", "notify", "error"],
] as const)(
  "shows the current %s stage independently of a previous completion",
  async (phase, step, status) => {
    mocks.load.mockResolvedValue({
      ok: true,
      result: {
        ...state,
        configured: true,
        phase,
        last_execution: { status: "done", completed_at: "2026-10-22T00:00:00Z" },
      },
    });
    await render({ setup: false });
    const current = host.querySelector('li[aria-current="step"]')!;
    expect(current.getAttribute("data-step")).toBe(step);
    expect(current.getAttribute("data-state")).toBe(status);
    expect(host.querySelector('li[data-step="completed"]')?.getAttribute("data-state")).toBe(
      "todo",
    );
    expect(Boolean(current.querySelector(".animate-spin"))).toBe(false);
    expect(mocks.change).not.toHaveBeenCalled();
  },
);

it("shows only the current delivery stage in a compact pane", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, configured: true, phase: "delivered" },
  });
  await render({ setup: false, compact: true });
  expect(host.querySelectorAll("li[data-step]")).toHaveLength(1);
  expect(host.querySelector('li[aria-current="step"]')?.textContent).toContain(
    "grokActor.steps.waiting",
  );
  expect(mocks.change).not.toHaveBeenCalled();
});

it.each(["ready", "delivered", "working", "interrupted"])(
  "keeps %s overview out of Bot connection editing without losing credentials or recovery",
  async (phase) => {
    const unfinished = phase === "working" || phase === "interrupted";
    mocks.load.mockResolvedValue({
      ok: true,
      result: {
        ...state,
        configured: true,
        phase,
        active_turn_id: unfinished ? "accepted-turn" : undefined,
        last_execution: { status: "done", completed_at: "2026-10-22T00:00:00Z" },
      },
    });
    await render();
    expect(host.querySelector("dl")).toBeNull();
    expect(host.querySelector("li[data-step]")).toBeNull();
    expect(host.textContent).not.toContain(`grokWebhook.phase.${phase}`);
    expect(host.textContent).not.toContain("grokActor.outcomes.done");
    expect(host.querySelectorAll("input[type=password]")).toHaveLength(2);
    if (unfinished) {
      expect(host.textContent).toContain("grokWebhook.waitForTask");
      expect(button("nativeActor.endTurn")).toBeDefined();
    }
    expect(mocks.change).not.toHaveBeenCalled();
  },
);
