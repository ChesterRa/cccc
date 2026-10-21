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
it("requires a saved Bot but allows editing and applying while enabled and idle", async () => {
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, bound: false } });
  await render();
  expect(button("grokWebhook.instructions").disabled).toBe(true);
  expect(host.querySelector("input")!.disabled).toBe(true);
  mocks.load.mockResolvedValue({ ok: true, result: { ...state, enabled: true } });
  await tick();
  expect(host.querySelector("input")!.disabled).toBe(false);
  expect(button("grokWebhook.pause")).toBeUndefined();
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
  expect(host.textContent).toContain("grokActor.configured");
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
it("retains exact unfinished task IDs across confirmation and polling", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, phase: "interrupted", active_turn_id: "older", active_event_ids: ["old"] },
  });
  await render();
  await act(async () => button("grokWebhook.resolve").click());
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, phase: "interrupted", active_turn_id: "newer", active_event_ids: ["new"] },
  });
  await tick();
  await act(async () => button("grokWebhook.confirmResolve").click());
  expect(mocks.change).toHaveBeenCalledExactlyOnceWith("g", "bot", {
    action: "resolve",
    turn_id: "older",
    event_ids: ["old"],
  });
});
it("can stop unfinished work after the connector was revoked", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      ...state,
      connected: false,
      enabled: true,
      phase: "interrupted",
      active_turn_id: "task",
    },
  });
  await render();
  await act(async () => button("grokWebhook.pause").click());
  expect(mocks.stop).toHaveBeenCalledExactlyOnceWith("g", "bot");
});
it("shows wake acceptance separately from confirmed completion", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: { ...state, configured: true, phase: "delivered" },
  });
  await render();
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
