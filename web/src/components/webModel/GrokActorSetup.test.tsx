// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
const mocks = vi.hoisted(() => ({ load: vi.fn(), remove: vi.fn(), stop: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
  fetchWebModelBrowserSurfaceSession: (...a: unknown[]) => mocks.load(...a),
  changeWebModelPairing: (...a: unknown[]) => mocks.remove(...a),
  stopActor: (...a: unknown[]) => mocks.stop(...a),
}));
vi.mock("../browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: () => null,
}));
import { GrokActorSetup } from "./GrokActorSetup";
const bot = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
const next = "https://grok.com/bot/958f2446-013f-4225-8dd3-f146295992d7";
const response = (enabled = false, url = "") => ({
  ok: true,
  result: {
    pairing: { state: url ? "bound" : "unpaired", actor_enabled: enabled, url },
    browser_session: {},
    browser_surface: {},
  },
});
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
const button = (key: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === key)!;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mocks.load.mockResolvedValue(response(true, bot));
  mocks.stop.mockResolvedValue({ ok: true });
  mocks.remove.mockResolvedValue({ ok: true });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});
function Fixture() {
  const [draft, setDraft] = useState<string>();
  return (
    <GrokActorSetup
      groupId="g_a"
      actorId="A"
      isDark={false}
      draftUrl={draft}
      onDraftChange={setDraft}
    />
  );
}
async function mount() {
  await act(async () => root.render(<Fixture />));
}
async function fill(value: string) {
  await act(async () => {
    const input = host.querySelector("input")!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
it("keeps a running Actor's edits local across polls and lets Cancel discard them", async () => {
  await mount();
  await act(async () => button("grokActor.change").click());
  expect(host.querySelector("input")!.disabled).toBe(false);
  await fill(next);
  await act(async () => vi.advanceTimersByTime(2000));
  expect(host.querySelector("input")!.value).toBe(next);
  expect(mocks.stop).not.toHaveBeenCalled();
  expect(mocks.remove).not.toHaveBeenCalled();
  expect(button("grokActor.save")).toBeUndefined();
  await act(async () => button("common:cancel").click());
  await act(async () => button("grokActor.change").click());
  expect(host.querySelector("input")!.value).toBe(bot);
});
it("waits for initial status, then permits running-Actor editing", async () => {
  let finish!: (v: ReturnType<typeof response>) => void;
  mocks.load.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  await mount();
  expect(host.querySelector("input")!.disabled).toBe(true);
  await act(async () => finish(response(true)));
  expect(host.querySelector("input")!.disabled).toBe(false);
});
it("confirms disconnect in place, stops only this Actor and ignores a stale poll", async () => {
  await mount();
  let finish!: (v: ReturnType<typeof response>) => void;
  mocks.load.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  await act(async () => vi.advanceTimersByTime(2000));
  await act(async () => button("grokActor.remove").click());
  expect(mocks.stop).not.toHaveBeenCalled();
  mocks.load.mockResolvedValue(response(false));
  await act(async () => button("grokActor.confirmRemove").click());
  expect(mocks.stop).toHaveBeenCalledExactlyOnceWith("g_a", "A");
  expect(mocks.remove).toHaveBeenCalledExactlyOnceWith("g_a", "A", "remove");
  expect(mocks.stop.mock.invocationCallOrder[0]).toBeLessThan(
    mocks.remove.mock.invocationCallOrder[0],
  );
  await act(async () => finish(response(true, bot)));
  expect(host.textContent).not.toContain(bot);
});
it("keeps the binding and draft when stopping fails", async () => {
  await mount();
  await act(async () => button("grokActor.change").click());
  await fill(next);
  mocks.stop.mockResolvedValue({ ok: false, error: { message: "stop failed" } });
  await act(async () => button("grokActor.remove").click());
  await act(async () => button("grokActor.confirmRemove").click());
  expect(mocks.remove).not.toHaveBeenCalled();
  expect(host.querySelector("input")!.value).toBe(next);
  expect(host.textContent).toContain("stop failed");
});
