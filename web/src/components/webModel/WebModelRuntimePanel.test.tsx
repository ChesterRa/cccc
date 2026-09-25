// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../../types";

const mocks = vi.hoisted(() => ({ load: vi.fn(), open: vi.fn(), edit: vi.fn() }));
vi.mock("react-i18next", () => {
  const t = (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../../stores", () => ({
  useModalStore: (select: (s: unknown) => unknown) => select({ openActorEditor: mocks.edit }),
}));
vi.mock("../../services/api", () => ({
  fetchWebModelBrowserSession: (...a: unknown[]) => mocks.load(...a),
  fetchWebModelBrowserSurfaceSession: (...a: unknown[]) => mocks.load(...a),
  openWebModelBrowserSurfaceSession: (...a: unknown[]) => mocks.open(...a),
  getWebModelBrowserSurfaceWebSocketUrl: () => "ws://localhost/actor",
}));
import { WebModelRuntimePanel } from "./WebModelRuntimePanel";

const bot = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
const actor = { id: "a", runtime: "grok_web_model" } as Actor;
const response = (url = bot, active = true) => ({
  ok: true as const,
  result: {
    pairing: { state: url ? "bound" : "unpaired", actor_enabled: true, url },
    browser_session: { active },
    browser_surface: { active, state: active ? "ready" : "idle", url },
  },
});
class Socket {
  static OPEN = 1;
  static CONNECTING = 0;
  static instances: Socket[] = [];
  readyState = 1;
  constructor() {
    Socket.instances.push(this);
  }
  close() {
    this.readyState = 3;
  }
}
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.useFakeTimers();
  Socket.instances = [];
  vi.stubGlobal("WebSocket", Socket);
  mocks.load.mockResolvedValue(response());
  mocks.open.mockResolvedValue(response());
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
async function mount(current = actor) {
  await act(async () =>
    root.render(
      <WebModelRuntimePanel groupId="g_a" actor={current} isRunning isVisible isDark={false} />,
    ),
  );
}
it("requires a saved Grok Bot URL before opening any browser", async () => {
  mocks.load.mockResolvedValue(response("", false));
  await mount();
  expect(mocks.open).not.toHaveBeenCalled();
  expect(Socket.instances).toHaveLength(0);
  expect(host.textContent).toContain("settings:grokActor.urlRequired");
  expect(host.textContent).not.toContain("https://grok.com/");
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent?.includes("settings:grokActor.title"))!
      .click(),
  );
  expect(mocks.edit).toHaveBeenCalledWith(actor, "chatgpt");
});
it("waits for the owned window's open operation before attaching to a warmup surface", async () => {
  let finish!: (r: ReturnType<typeof response>) => void;
  mocks.open.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await mount();
  expect(mocks.open).toHaveBeenCalledTimes(1);
  expect(Socket.instances).toHaveLength(0);
  await act(async () => finish(response()));
  expect(Socket.instances).toHaveLength(1);
  expect(host.textContent).toContain(bot);
});
it("rechecks the browser when the same Actor changes provider", async () => {
  await mount();
  await mount({ ...actor, runtime: "web_model" });
  expect(mocks.open).toHaveBeenCalledTimes(2);
  expect(Socket.instances[0].readyState).toBe(3);
});
it("reports an open failure instead of attaching to the earlier warmup snapshot", async () => {
  mocks.open.mockResolvedValue({
    ok: false,
    error: { code: "open_failed", message: "navigation timed out" },
  });
  await mount();
  expect(Socket.instances).toHaveLength(0);
  expect(host.textContent).toContain("navigation timed out");
});
it("does not attach when the surface disappeared before the open response", async () => {
  mocks.open.mockResolvedValue(response(bot, false));
  await mount();
  expect(Socket.instances).toHaveLength(0);
});
