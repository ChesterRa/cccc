// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../../types";

const mocks = vi.hoisted(() => ({ load: vi.fn(), open: vi.fn(), edit: vi.fn() }));
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t, i18n: { language: "en" } }) };
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

const bot = "https://chatgpt.com/c/fixture";
const actor = { id: "a", runtime: "web_model" } as Actor;
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
  send() {}
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

it("keeps administrator-owned browser state out of a Group user's panel", async () => {
  await act(async () =>
    root.render(
      <WebModelRuntimePanel
        groupId="g_a"
        actor={actor}
        isRunning
        isVisible
        isDark={false}
        canAccessConnection={false}
      />,
    ),
  );
  await act(async () => vi.advanceTimersByTimeAsync(6000));
  expect(mocks.load).not.toHaveBeenCalled();
  expect(mocks.open).not.toHaveBeenCalled();
  expect(Socket.instances).toHaveLength(0);
  expect(host.textContent).toContain("webModelActor.adminManaged");
  expect(host.querySelectorAll("button")).toHaveLength(0);
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
it("rechecks the browser when the selected Actor changes", async () => {
  await mount();
  await mount({ ...actor, id: "other" });
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
it("keeps conversation settings reachable from the compact toolbar without opening another surface", async () => {
  await act(async () =>
    root.render(
      <WebModelRuntimePanel
        groupId="g_a"
        actor={actor}
        isRunning
        isVisible
        isDark={false}
        compact
      />,
    ),
  );
  const opened = mocks.open.mock.calls.length;
  expect(host.textContent).not.toContain("settings:webModelActor.title");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="settings:title"]')!.click(),
  );
  const edit = [...document.querySelectorAll("button")].find((b) =>
    b.textContent?.includes("settings:webModelActor.title"),
  );
  expect(edit).toBeTruthy();
  await act(async () => edit!.click());
  expect(mocks.edit).toHaveBeenCalledWith(actor, "chatgpt");
  expect(mocks.open).toHaveBeenCalledTimes(opened);
});

it("does not offer an empty settings menu for read-only panes", async () => {
  await act(async () =>
    root.render(
      <WebModelRuntimePanel
        groupId="g_a"
        actor={actor}
        isRunning
        isVisible
        isDark={false}
        compact
        readOnly
      />,
    ),
  );
  expect(host.querySelector('[aria-label="settings:title"]')).toBeNull();
  expect(mocks.open).not.toHaveBeenCalled();
});

it("keeps the normal detail toolbar free of a duplicate Actor editor", async () => {
  await mount();
  expect(host.textContent).not.toContain("settings:webModelActor.title");
  expect(mocks.edit).not.toHaveBeenCalled();
});

it("checks a paused Actor's existing page without opening, refreshing or sending", async () => {
  const notReady = response();
  Object.assign(notReady.result.browser_session, {
    ready: false,
    login_required: false,
    state: "page_not_ready",
  });
  mocks.load.mockResolvedValue(notReady);
  const shared = vi.fn();
  await act(async () =>
    root.render(
      <WebModelRuntimePanel
        groupId="g_a"
        actor={actor}
        isRunning={false}
        isVisible
        isDark={false}
        onOpenSharedSettings={shared}
      />,
    ),
  );
  expect(host.textContent).toContain("webModelActor.states.page_not_ready");
  const buttons = () => [...host.querySelectorAll("button")];
  await act(async () =>
    buttons()
      .find((b) => b.textContent === "settings:webModelActor.sharedLogin")!
      .click(),
  );
  expect(shared).toHaveBeenCalledOnce();
  let finishRead!: (r: ReturnType<typeof response>) => void;
  const ready = response();
  Object.assign(ready.result.browser_session, { ready: true, checked_at: "2026-10-22T12:00:00Z" });
  mocks.load.mockImplementation((_group, _actor, options) =>
    options?.inspect
      ? Promise.resolve(ready)
      : new Promise((resolve) => {
          finishRead = resolve;
        }),
  );
  await act(async () => vi.advanceTimersByTime(2000));
  await act(async () =>
    buttons()
      .find((b) => b.textContent === "settings:webModelActor.checkPage")!
      .click(),
  );
  expect(mocks.load).toHaveBeenCalledWith("g_a", "a", { inspect: true });
  await act(async () => finishRead(notReady));
  expect(host.textContent).not.toContain("webModelActor.states.page_not_ready");
  expect(
    buttons().find((b) => b.textContent === "settings:webModelActor.checkPage")?.disabled,
  ).toBe(false);
  expect(mocks.open).not.toHaveBeenCalled();
  expect(Socket.instances).toHaveLength(0);
});

it("keeps an explicit check newer than a pending surface reconnection status read", async () => {
  const notReady = response();
  Object.assign(notReady.result.browser_session, {
    ready: false,
    login_required: false,
    state: "page_not_ready",
  });
  mocks.load.mockResolvedValue(notReady);
  mocks.open.mockResolvedValue(notReady);
  await mount();
  let finishRead!: (r: ReturnType<typeof response>) => void;
  const ready = response();
  Object.assign(ready.result.browser_session, { ready: true });
  mocks.load.mockImplementation((_group, _actor, options) =>
    options?.inspect
      ? Promise.resolve(ready)
      : new Promise((resolve) => {
          finishRead = resolve;
        }),
  );
  await act(async () => (Socket.instances[0] as Socket & { onclose: () => void }).onclose());
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent === "settings:webModelActor.checkPage")!
      .click(),
  );
  expect(host.textContent).not.toContain("webModelActor.states.page_not_ready");
  await act(async () => finishRead(notReady));
  expect(host.textContent).not.toContain("webModelActor.states.page_not_ready");
  expect(mocks.open).toHaveBeenCalledOnce();
});
