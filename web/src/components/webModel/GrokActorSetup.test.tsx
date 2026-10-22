// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
const mocks = vi.hoisted(() => ({ load: vi.fn(), remove: vi.fn(), stop: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
  fetchGrokActorState: (...a: unknown[]) => mocks.load(...a),
  changeGrokWebhook: (...a: unknown[]) => mocks.remove(...a),
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
    group_id: "g_a",
    actor_id: "A",
    connected: true,
    bound: Boolean(url),
    configured: false,
    enabled,
    bot_url: url,
    phase: "unconfigured",
    pending_count: 0,
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
it("keeps shared settings reachable without exposing browser controls", async () => {
  const open = vi.fn();
  await act(async () =>
    root.render(
      <GrokActorSetup
        groupId="g_a"
        actorId="A"
        isDark={false}
        onDraftChange={() => {}}
        onOpenSharedSettings={open}
      />,
    ),
  );
  await act(async () => button("grokActor.shared").click());
  expect(open).toHaveBeenCalledOnce();
  expect(host.querySelector("select")).toBeNull();
  expect(mocks.remove).not.toHaveBeenCalled();
});
it("unlocks Routine setup when the saved Bot URL is confirmed without discarding a different draft", async () => {
  await mount();
  await act(async () => button("grokActor.change").click());
  await fill(next);
  expect(button("grokWebhook.instructions").disabled).toBe(true);
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(button("grokWebhook.instructions").disabled).toBe(true);
  mocks.load.mockResolvedValue(response(true, next));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(button("grokWebhook.instructions").disabled).toBe(false);
  expect(host.querySelector("input")!.value).toBe(next);
});
it.each([
  `${next}?from=fixture`,
  `${next}#routine`,
  `https://grok.com/bot/${next.split("/").at(-1)!.toUpperCase()}`,
  `https://grok.com/bot/${next.split("/").at(-1)!.toUpperCase()}?from=fixture#routine`,
])("unlocks Routine setup after the daemon canonicalizes %s", async (draft) => {
  await mount();
  await act(async () => button("grokActor.change").click());
  await fill(draft);
  expect(button("grokWebhook.instructions").disabled).toBe(true);
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(button("grokWebhook.instructions").disabled).toBe(true);
  mocks.load.mockResolvedValue(response(true, next));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(button("grokWebhook.instructions").disabled).toBe(false);
  expect(host.querySelector("input")!.value).toBe(next);
});
it("retains an invalid URL draft while the Actor has no saved Bot", async () => {
  mocks.load.mockResolvedValue(response(false));
  await mount();
  await fill("not-a-bot-url");
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(host.querySelector("input")!.value).toBe("not-a-bot-url");
  expect(button("grokWebhook.instructions").disabled).toBe(true);
});
