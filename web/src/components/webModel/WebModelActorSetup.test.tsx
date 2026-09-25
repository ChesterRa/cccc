// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  stop: vi.fn(),
  navigate: vi.fn(),
  pair: vi.fn(),
  start: vi.fn(),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
  }),
}));
vi.mock("../../services/api", () => ({
  fetchWebModelBrowserSurfaceSession: (...a: unknown[]) => mocks.load(...a),
  stopActor: (...a: unknown[]) => mocks.stop(...a),
  startActor: (...a: unknown[]) => mocks.start(...a),
  bindCurrentWebModelBrowserConversation: (...a: unknown[]) => mocks.navigate(...a),
  changeWebModelPairing: (...a: unknown[]) => mocks.pair(...a),
  getWebModelBrowserSurfaceWebSocketUrl: () => "ws://fixture",
}));
vi.mock("../browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: () => <div>owned-viewer</div>,
}));
import { WebModelActorSetup } from "./WebModelActorSetup";
const url = "https://chatgpt.com/c/fixture-new";
const response = (enabled = true, state = "bound", pairingId = "") => ({
  ok: true,
  result: {
    pairing: {
      state,
      url: state === "bound" && pairingId ? url : "https://chatgpt.com/c/fixture-old",
      actor_enabled: enabled,
      pairing_id: pairingId,
    },
    browser_session: { active: true },
  },
});
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
const button = (key: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === `webModelActor.${key}`)!;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mocks.load.mockResolvedValue(response());
  mocks.stop.mockResolvedValue({ ok: true });
  mocks.navigate.mockResolvedValue({ ok: true });
  mocks.start.mockResolvedValue({ ok: true });
  vi.stubGlobal("confirm", vi.fn().mockReturnValue(true));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
async function edit() {
  await act(async () =>
    root.render(<WebModelActorSetup groupId="g_a" actorId="A" isDark={false} />),
  );
  await act(async () => button("changeConversation").click());
  const input = host.querySelector("input")!;
  expect(input.disabled).toBe(false);
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, url);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
it("permits local URL edits while running; declining pause changes nothing", async () => {
  await edit();
  expect(mocks.stop).not.toHaveBeenCalled();
  vi.mocked(window.confirm).mockReturnValue(false);
  await act(async () => button("openUrl").click());
  expect(mocks.stop).not.toHaveBeenCalled();
  expect(mocks.navigate).not.toHaveBeenCalled();
  expect(host.querySelector("input")!.value).toBe(url);
});
it("pauses this Actor only at preview and preserves a rejected page draft", async () => {
  await edit();
  mocks.navigate.mockResolvedValue({
    ok: false,
    error: { code: "composer_occupied", message: "composer_occupied" },
  });
  await act(async () => button("openUrl").click());
  expect(mocks.stop).toHaveBeenCalledExactlyOnceWith("g_a", "A");
  expect(mocks.navigate).toHaveBeenCalledExactlyOnceWith({
    groupId: "g_a",
    actorId: "A",
    conversationUrl: url,
    newChat: false,
  });
  expect(mocks.stop.mock.invocationCallOrder[0]).toBeLessThan(
    mocks.navigate.mock.invocationCallOrder[0],
  );
  expect(host.querySelector("input")!.value).toBe(url);
  expect(host.textContent).toContain("composer_occupied");
  expect(mocks.pair).not.toHaveBeenCalled();
  expect(mocks.start).not.toHaveBeenCalled();
});
it("never navigates if the Actor could not stop", async () => {
  await edit();
  mocks.stop.mockResolvedValue({ ok: false, error: { message: "stop failed" } });
  await act(async () => button("openUrl").click());
  expect(mocks.navigate).not.toHaveBeenCalled();
  expect(host.textContent).toContain("stop failed");
});
it("keeps connection verification separate and exposes Start only after this attempt succeeds", async () => {
  await edit();
  mocks.load.mockResolvedValue(response(false));
  await act(async () => button("openUrl").click());
  mocks.pair.mockResolvedValue({
    ok: true,
    result: { state: "waiting", pairing_id: "fixture-pair" },
  });
  mocks.load.mockResolvedValue(response(false, "waiting", "fixture-pair"));
  await act(async () => button("useConversation").click());
  expect(button("start")).toBeUndefined();
  expect(button("cancel")).toBeDefined();
  mocks.load.mockResolvedValue(response(false, "bound", "fixture-pair"));
  await act(async () => vi.advanceTimersByTime(2000));
  expect(button("start")).toBeDefined();
  expect(mocks.start).not.toHaveBeenCalled();
  await act(async () => button("start").click());
  expect(mocks.start).toHaveBeenCalledExactlyOnceWith("g_a", "A");
});
