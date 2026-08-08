// @vitest-environment happy-dom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { PresentationBrowserSurfaceState } from "../../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
  }),
}));

vi.mock("@novnc/novnc", () => ({
  default: class FakeRfb {
    viewOnly = false;
    scaleViewport = false;
    resizeSession = false;
    clipViewport = false;
    background = "";

    disconnect() {}

    addEventListener(type: string, listener: (event: Event) => void) {
      if (type === "connect") listener(new Event("connect"));
    }
  },
}));

import { ProjectedBrowserSurfacePanel } from "./ProjectedBrowserSurfacePanel";

class FakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static instances: FakeWebSocket[] = [];

  readonly url: string;
  readyState = FakeWebSocket.OPEN;
  onopen: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];

  constructor(url: string | URL) {
    this.url = String(url);
    FakeWebSocket.instances.push(this);
  }

  send(data: string) {
    this.sent.push(String(data));
  }

  close() {
    this.readyState = 3;
  }
}

const readySurface: PresentationBrowserSurfaceState = {
  active: true,
  state: "ready",
  message: "Ready",
  error: null,
  strategy: "system_browser_cdp:chrome_xvfb",
  url: "https://example.test/",
  width: 1280,
  height: 800,
  started_at: "2026-08-08T00:00:00Z",
  updated_at: "2026-08-08T00:00:00Z",
  last_frame_seq: 0,
  last_frame_at: "",
  controller_attached: false,
  metadata: { display_owned: true, display_owner: "cccc_xvfb" },
  viewer: { kind: "vnc", vnc: { available: true } },
};

describe("ProjectedBrowserSurfacePanel viewer switching", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    vi.stubGlobal(
      "Image",
      class {
        onload: (() => void) | null = null;

        set src(_value: string) {
          this.onload?.();
        }
      },
    );
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  it("switches transports without starting a replacement browser session", async () => {
    const loadSession = vi.fn(async () => ({
      ok: true as const,
      result: { browser_surface: readySurface },
    }));
    const startSession = vi.fn(async () => ({
      ok: true as const,
      result: { browser_surface: readySurface },
    }));

    await act(async () => {
      root.render(
        <ProjectedBrowserSurfacePanel
          isDark={false}
          refreshNonce={0}
          reuseActiveSession={false}
          sessionIdentity="presentation:slot:example"
          defaultViewerMode="page"
          loadSession={loadSession}
          startSession={startSession}
          webSocketUrl="ws://localhost/browser"
        />,
      );
    });

    expect(startSession).toHaveBeenCalledTimes(1);
    expect(FakeWebSocket.instances.at(-1)?.url).toContain("viewer_mode=screencast");

    const browserButton = Array.from(host.querySelectorAll("button")).find(
      (button) => button.textContent === "Browser",
    );
    expect(browserButton).toBeTruthy();

    await act(async () => {
      browserButton?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(loadSession).toHaveBeenCalledTimes(2);
    expect(startSession).toHaveBeenCalledTimes(1);
    expect(FakeWebSocket.instances.at(-1)?.url).toContain("viewer_mode=auto");
  });

  it("sends a native wheel target mapped into page coordinates", async () => {
    const loadSession = vi.fn(async () => ({
      ok: true as const,
      result: { browser_surface: readySurface },
    }));
    const startSession = vi.fn(async () => ({
      ok: true as const,
      result: { browser_surface: readySurface },
    }));

    await act(async () => {
      root.render(
        <ProjectedBrowserSurfacePanel
          isDark={false}
          refreshNonce={0}
          defaultViewerMode="page"
          loadSession={loadSession}
          startSession={startSession}
          webSocketUrl="ws://localhost/browser"
        />,
      );
    });

    const socket = FakeWebSocket.instances.at(-1);
    await act(async () => {
      socket?.onmessage?.({
        data: JSON.stringify({
          t: "frame",
          seq: 1,
          data_base64: "AA==",
          width: 1000,
          height: 500,
          mime: "image/jpeg",
        }),
      } as MessageEvent);
      await Promise.resolve();
    });

    const frame = host.querySelector("img");
    expect(frame).toBeTruthy();
    vi.spyOn(frame!, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      right: 1000,
      bottom: 600,
      width: 1000,
      height: 600,
      toJSON: () => ({}),
    });

    const wheel = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaX: 4.4,
      deltaY: 120.6,
    });
    Object.defineProperties(wheel, { clientX: { value: 500 }, clientY: { value: 300 } });
    await act(async () => {
      frame?.dispatchEvent(wheel);
    });

    expect(wheel.defaultPrevented).toBe(true);
    expect(JSON.parse(socket?.sent.at(-1) || "{}")).toEqual({
      t: "scroll",
      x: 500,
      y: 250,
      dx: 4,
      dy: 121,
    });
  });
});
