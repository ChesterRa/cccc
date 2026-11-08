// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { PresentationBrowserSurfaceState } from "../../types";
import { ProjectedBrowserSurfacePanel } from "./ProjectedBrowserSurfacePanel";
import { PresentationBrowserSurfacePanel } from "../presentation/PresentationBrowserSurfacePanel";
import * as api from "../../services/api";

const group = vi.hoisted(() => ({
  selectedGroupId: "g",
  groupPresentation: {
    v: 1,
    slots: [
      {
        slot_id: "slot-1",
        index: 1,
        card: { card_type: "web_preview", content: { url: "https://example.test/new" } },
      },
    ],
  },
  refreshPresentation: vi.fn(),
}));
vi.mock("../../stores/useGroupStore", () => ({ useGroupStore: { getState: () => group } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
  }),
}));
vi.mock("../../services/api", () => ({
  fetchPresentationBrowserSurfaceSession: vi.fn(),
  startPresentationBrowserSurfaceSession: vi.fn(),
  getPresentationBrowserSurfaceWebSocketUrl: () => "ws://fixture.invalid/browser",
  refreshAuthTokenInUrl: (url: string) => url,
  getPresentationAssetUrl: () => "",
}));

class FakeSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  readyState = FakeSocket.OPEN;
  onopen: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];
  constructor(_url: string | URL) {
    FakeSocket.instances.push(this);
  }
  send(value: string) {
    this.sent.push(value);
  }
  close() {
    this.readyState = 3;
  }
}

const ready: PresentationBrowserSurfaceState = {
  active: true,
  state: "ready",
  message: "Ready",
  error: null,
  strategy: "fixture",
  url: "https://example.test/",
  width: 960,
  height: 640,
  started_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_frame_seq: 0,
  last_frame_at: "",
  controller_attached: false,
  viewer: { kind: "screencast" },
};

let host: HTMLDivElement;
let root: Root;
let size: { width: number; height: number };
let notifyResize: (() => void) | undefined;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  FakeSocket.instances = [];
  group.selectedGroupId = "g";
  group.groupPresentation.slots[0].card.content.url = "https://example.test/new";
  group.refreshPresentation.mockResolvedValue(true);
  vi.stubGlobal("WebSocket", FakeSocket);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        notifyResize = () => callback([], this as unknown as ResizeObserver);
      }
      observe() {}
      disconnect() {}
    },
  );
  size = { width: 0, height: 0 };
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => size.width);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(() => size.height);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.resetAllMocks();
  vi.unstubAllGlobals();
});

it("does not resize a hidden or zero-sized viewer, and resumes the same session", async () => {
  let surface = ready;
  const loadSession = vi.fn(async () => ({
    ok: true as const,
    result: { browser_surface: surface },
  }));
  const startSession = vi.fn(async () => ({
    ok: true as const,
    result: { browser_surface: ready },
  }));
  const show = (visible: boolean) =>
    act(async () =>
      root.render(
        <ProjectedBrowserSurfacePanel
          isDark={false}
          visible={visible}
          refreshNonce={0}
          reuseActiveSession={false}
          sessionIdentity="g:slot-1"
          loadSession={loadSession}
          startSession={startSession}
          webSocketUrl="ws://fixture.invalid/browser"
        />,
      ),
    );
  await show(false);
  expect(startSession).not.toHaveBeenCalled();
  await show(true);
  expect(startSession).toHaveBeenCalledTimes(1);
  const first = FakeSocket.instances[0];
  await act(async () => vi.advanceTimersByTime(150));
  expect(first.sent).toEqual([]); // 0x0 must not become a 640x480 shared viewport.
  size = { width: 800, height: 600 };
  await act(async () => {
    notifyResize?.();
    vi.advanceTimersByTime(150);
  });
  expect(first.sent.map((item) => JSON.parse(item))).toContainEqual({
    t: "resize",
    width: 800,
    height: 600,
  });
  await show(false);
  expect(first.readyState).toBe(3);
  await show(true);
  expect(startSession).toHaveBeenCalledTimes(1);
  expect(FakeSocket.instances).toHaveLength(2);

  await show(false);
  surface = { ...ready, active: false, state: "closed", message: "Closed by another viewer" };
  await show(true);
  expect(startSession).toHaveBeenCalledTimes(1);
  expect(FakeSocket.instances).toHaveLength(2);
  expect(host.textContent).toContain("Closed by another viewer");
});

it.each(["before", "after"] as const)(
  "joins an in-flight start if the tab returns %s the start completes",
  async (returns) => {
    let finish!: (value: {
      ok: true;
      result: { browser_surface: PresentationBrowserSurfaceState };
    }) => void;
    const started = new Promise<{
      ok: true;
      result: { browser_surface: PresentationBrowserSurfaceState };
    }>((resolve) => {
      finish = resolve;
    });
    const loadSession = vi.fn(async () => ({
      ok: true as const,
      result: { browser_surface: ready },
    }));
    const startSession = vi.fn(() => started);
    const show = (visible: boolean) =>
      act(async () =>
        root.render(
          <ProjectedBrowserSurfacePanel
            isDark={false}
            visible={visible}
            refreshNonce={0}
            reuseActiveSession={false}
            sessionIdentity="g:slot-1"
            loadSession={loadSession}
            startSession={startSession}
            webSocketUrl="ws://fixture.invalid/browser"
          />,
        ),
      );
    await show(true);
    expect(startSession).toHaveBeenCalledTimes(1);
    await show(false);
    if (returns === "before") await show(true);
    await act(async () => {
      finish({ ok: true, result: { browser_surface: ready } });
      await started;
    });
    if (returns === "after") await show(true);
    expect(startSession).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(1);
  },
);

it("reattaches a resource viewer to the shared slot after its Group is remounted", async () => {
  vi.mocked(api.fetchPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: true,
    result: { group_id: "g", browser_surface: ready },
  });
  const viewer = () => (
    <PresentationBrowserSurfacePanel
      groupId="g"
      slotId="slot-1"
      url="https://example.test/"
      isDark={false}
      refreshNonce={0}
      reuseExistingSession
    />
  );
  await act(async () => root.render(viewer()));
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
  expect(FakeSocket.instances).toHaveLength(1);
  await act(async () => root.render(null));
  expect(FakeSocket.instances[0].readyState).toBe(3);
  await act(async () => root.render(viewer()));
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
  expect(FakeSocket.instances).toHaveLength(2);
});

it("retries a rejected in-flight start after the user leaves and returns", async () => {
  let fail!: (error: Error) => void;
  const firstStart = new Promise<{
    ok: true;
    result: { browser_surface: PresentationBrowserSurfaceState };
  }>((_resolve, reject) => {
    fail = reject;
  });
  const loadSession = vi.fn(async () => ({
    ok: true as const,
    result: { browser_surface: ready },
  }));
  const startSession = vi
    .fn()
    .mockReturnValueOnce(firstStart)
    .mockResolvedValueOnce({ ok: true, result: { browser_surface: ready } });
  const show = (visible: boolean) =>
    act(async () =>
      root.render(
        <ProjectedBrowserSurfacePanel
          isDark={false}
          visible={visible}
          refreshNonce={0}
          reuseActiveSession={false}
          sessionIdentity="g:slot-1"
          loadSession={loadSession}
          startSession={startSession}
          webSocketUrl="ws://fixture.invalid/browser"
        />,
      ),
    );
  await show(true);
  await show(false);
  await show(true);
  await act(async () => {
    fail(new Error("fixture start failed"));
  });
  expect(host.textContent).toContain("fixture start failed");
  expect(startSession).toHaveBeenCalledTimes(1);
  await show(false);
  await show(true);
  expect(startSession).toHaveBeenCalledTimes(2);
  expect(FakeSocket.instances).toHaveLength(1);
});

it("asks before replacing a shared session for a different published URL", async () => {
  const original = { ...ready, initial_url: "https://example.test/old" };
  const replacement = { ...ready, initial_url: "https://example.test/new" };
  vi.mocked(api.fetchPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: true,
    result: { group_id: "g", browser_surface: original },
  });
  vi.mocked(api.startPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: true,
    result: { group_id: "g", browser_surface: replacement },
  });
  await act(async () =>
    root.render(
      <PresentationBrowserSurfacePanel
        groupId="g"
        slotId="slot-1"
        url="https://example.test/new"
        isDark={false}
        refreshNonce={0}
        reuseExistingSession
      />,
    ),
  );
  expect(host.textContent).toContain("presentationBrowserContentChanged");
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
  expect(FakeSocket.instances).toHaveLength(0);
  const button = [...host.querySelectorAll("button")].find((element) =>
    element.textContent?.includes("presentationBrowserOpenNewContent"),
  );
  expect(button).toBeDefined();
  await act(async () => button!.click());
  expect(api.startPresentationBrowserSurfaceSession).toHaveBeenCalledWith(
    "g",
    expect.objectContaining({ url: "https://example.test/new", reuseExistingSession: false }),
  );
  expect(FakeSocket.instances).toHaveLength(1);
  expect(host.textContent).not.toContain("presentationBrowserContentChanged");

  // The initial URL on frames catches a replacement made after this viewer connected.
  await act(async () =>
    FakeSocket.instances[0].onmessage?.({
      data: JSON.stringify({ t: "frame", initial_url: "https://example.test/other" }),
    } as MessageEvent),
  );
  expect(host.textContent).toContain("presentationBrowserContentChanged");
  expect(FakeSocket.instances[0].readyState).toBe(3);
  expect(api.startPresentationBrowserSurfaceSession).toHaveBeenCalledTimes(1);
});

it("reuses a shared session after someone navigates inside the same published page", async () => {
  vi.mocked(api.fetchPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: true,
    result: {
      group_id: "g",
      browser_surface: {
        ...ready,
        initial_url: "https://example.test/start",
        url: "https://example.test/next-page",
      },
    },
  });
  await act(async () =>
    root.render(
      <PresentationBrowserSurfacePanel
        groupId="g"
        slotId="slot-1"
        url="https://example.test/start"
        isDark={false}
        refreshNonce={0}
        reuseExistingSession
      />,
    ),
  );
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
  expect(FakeSocket.instances).toHaveLength(1);
  expect(host.textContent).not.toContain("presentationBrowserContentChanged");
});

it("shows the same warning when a concurrent open reports a URL conflict", async () => {
  vi.mocked(api.fetchPresentationBrowserSurfaceSession)
    .mockResolvedValueOnce({
      ok: true,
      result: { group_id: "g", browser_surface: { active: false, state: "idle" } },
    })
    .mockResolvedValue({
      ok: true,
      result: {
        group_id: "g",
        browser_surface: { ...ready, initial_url: "https://example.test/old" },
      },
    });
  vi.mocked(api.startPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: false,
    error: { code: "presentation_browser_content_changed", message: "changed" },
  });
  await act(async () =>
    root.render(
      <PresentationBrowserSurfacePanel
        groupId="g"
        slotId="slot-1"
        url="https://example.test/new"
        isDark={false}
        refreshNonce={0}
        reuseExistingSession
      />,
    ),
  );
  expect(api.startPresentationBrowserSurfaceSession).toHaveBeenCalledWith(
    "g",
    expect.objectContaining({ reuseExistingSession: true }),
  );
  expect(host.textContent).toContain("presentationBrowserContentChanged");
  expect(FakeSocket.instances).toHaveLength(0);
});

it("waits for a successful card refresh before offering to replace a shared page", async () => {
  group.refreshPresentation.mockResolvedValueOnce(false).mockResolvedValue(true);
  vi.mocked(api.fetchPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: true,
    result: {
      group_id: "g",
      browser_surface: { ...ready, initial_url: "https://example.test/old" },
    },
  });
  await act(async () =>
    root.render(
      <PresentationBrowserSurfacePanel
        groupId="g"
        slotId="slot-1"
        url="https://example.test/new"
        isDark={false}
        refreshNonce={0}
        reuseExistingSession
      />,
    ),
  );
  expect(host.textContent).toContain("presentationBrowserRefreshFailed");
  expect(host.textContent).not.toContain("presentationBrowserOpenNewContent");
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
  const retry = [...host.querySelectorAll("button")].find((node) =>
    node.textContent?.includes("common:retry"),
  );
  await act(async () => retry!.click());
  expect(host.textContent).toContain("presentationBrowserContentChanged");
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
});

it("does not replace a page if the authoritative card changes before confirmation", async () => {
  vi.mocked(api.fetchPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: true,
    result: {
      group_id: "g",
      browser_surface: { ...ready, initial_url: "https://example.test/old" },
    },
  });
  await act(async () =>
    root.render(
      <PresentationBrowserSurfacePanel
        groupId="g"
        slotId="slot-1"
        url="https://example.test/new"
        isDark={false}
        refreshNonce={0}
        reuseExistingSession
      />,
    ),
  );
  expect(host.textContent).toContain("presentationBrowserContentChanged");
  group.refreshPresentation.mockImplementationOnce(async () => {
    group.groupPresentation.slots[0].card.content.url = "https://example.test/newer";
    return true;
  });
  const open = [...host.querySelectorAll("button")].find((node) =>
    node.textContent?.includes("presentationBrowserOpenNewContent"),
  );
  await act(async () => open!.click());
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
  expect(host.textContent).not.toContain("presentationBrowserOpenNewContent");
  expect(host.textContent).toContain("presentationBrowserStarting");
});

it("does not start a shared session when its current status cannot be read", async () => {
  vi.mocked(api.fetchPresentationBrowserSurfaceSession).mockResolvedValue({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "unavailable" },
  });
  await act(async () =>
    root.render(
      <PresentationBrowserSurfacePanel
        groupId="g"
        slotId="slot-1"
        url="https://example.test/new"
        isDark={false}
        refreshNonce={0}
        reuseExistingSession
      />,
    ),
  );
  expect(api.startPresentationBrowserSurfaceSession).not.toHaveBeenCalled();
  expect(host.textContent).toContain("unavailable");
});
