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
  onerror: (() => void) | null = null;
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

  it("shows an actionable hint when the websocket handshake is rejected", async () => {
    const loadSession = vi.fn(async () => ({
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
          webSocketUrl="ws://localhost/browser"
        />,
      );
    });

    await act(async () => {
      FakeWebSocket.instances.at(-1)?.onerror?.();
    });

    expect(host.textContent).toContain("reverse-proxy Origin headers");
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

  it("maps touch taps and drag scrolling into page coordinates", async () => {
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

    const pointer = (type: string, x: number, y: number, pointerId: number) => {
      const event = new Event(type, { bubbles: true, cancelable: true });
      Object.defineProperties(event, {
        button: { value: 0 },
        clientX: { value: x },
        clientY: { value: y },
        pointerId: { value: pointerId },
        pointerType: { value: "touch" },
      });
      frame?.dispatchEvent(event);
      return event;
    };

    await act(async () => {
      pointer("pointerdown", 500, 300, 7);
      pointer("pointermove", 500, 200, 7);
      pointer("pointerup", 500, 200, 7);
      pointer("pointerdown", 400, 250, 8);
      pointer("pointerup", 400, 250, 8);

      const rightClick = new Event("pointerdown", { bubbles: true, cancelable: true });
      Object.defineProperties(rightClick, {
        button: { value: 2 },
        clientX: { value: 300 },
        clientY: { value: 350 },
        pointerId: { value: 9 },
        pointerType: { value: "mouse" },
      });
      frame?.dispatchEvent(rightClick);
    });

    const inputRelay = host.querySelector<HTMLTextAreaElement>(
      "textarea[data-browser-input-relay]",
    );
    expect(inputRelay).toBeTruthy();
    await act(async () => {
      inputRelay?.dispatchEvent(new Event("compositionstart", { bubbles: true }));
      if (inputRelay) inputRelay.value = "你好";
      const composingInput = new Event("input", { bubbles: true });
      Object.defineProperty(composingInput, "isComposing", { value: true });
      inputRelay?.dispatchEvent(composingInput);
      inputRelay?.dispatchEvent(new Event("compositionend", { bubbles: true }));
      const committedInput = new Event("input", { bubbles: true });
      Object.defineProperty(committedInput, "isComposing", { value: false });
      inputRelay?.dispatchEvent(committedInput);

      if (inputRelay) inputRelay.value = "paste text";
      inputRelay?.dispatchEvent(new Event("input", { bubbles: true }));
    });

    const commands = (socket?.sent || []).map((value) => JSON.parse(value));
    expect(commands).toContainEqual({ t: "scroll", x: 500, y: 150, dx: 0, dy: 100 });
    expect(commands).toContainEqual({ t: "click", x: 400, y: 200, button: "left" });
    expect(commands).toContainEqual({ t: "click", x: 300, y: 300, button: "right" });
    expect(commands.filter((command) => command.t === "text")).toEqual([
      { t: "text", text: "你好" },
      { t: "text", text: "paste text" },
    ]);
  });
});
