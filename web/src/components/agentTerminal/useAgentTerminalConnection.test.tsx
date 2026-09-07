// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { Terminal } from "@xterm/xterm";
import { useAgentTerminalConnection } from "./useAgentTerminalConnection";

it("does not take over or resize a read-only attachment and limits explicit takeover to one attempt", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  const sockets: FakeSocket[] = [];
  class FakeSocket {
    static OPEN = 1;
    static CONNECTING = 0;
    readyState = 0;
    onopen?: () => void;
    onclose?: (event: { code: number }) => void;
    onmessage?: (event: { data: string | ArrayBuffer }) => void;
    sent: Uint8Array[] = [];
    constructor(public url: string) {
      sockets.push(this);
    }
    send(frame: Uint8Array) {
      this.sent.push(frame);
    }
    close() {
      this.readyState = 3;
    }
    attach(writable: boolean) {
      this.readyState = 1;
      this.onopen?.();
      this.onmessage?.({
        data: JSON.stringify({
          type: "terminal.attach",
          ok: true,
          result: { replay_cursor: 0, replay_end_cursor: 0, terminal_writable: writable },
        }),
      });
    }
  }
  vi.stubGlobal("WebSocket", FakeSocket);
  let input: (value: string) => void = () => {};
  let resize: (size: { cols: number; rows: number }) => void = () => {};
  const terminalRef = {
    current: {
      cols: 80,
      rows: 24,
      reset: vi.fn(),
      write: (_data: unknown, done?: () => void) => done?.(),
      onData: (cb: typeof input) => {
        input = cb;
        return { dispose: vi.fn() };
      },
      onResize: (cb: typeof resize) => {
        resize = cb;
        return { dispose: vi.fn() };
      },
    } as unknown as Terminal,
  };
  let controls: ReturnType<typeof useAgentTerminalConnection>;
  function Probe() {
    const [reconnectTrigger, setReconnectTrigger] = useState(0);
    controls = useAgentTerminalConnection({
      activated: true,
      isRunning: true,
      isHeadless: false,
      groupId: "g1",
      actorId: "a",
      actorRuntime: "codex",
      canControl: true,
      termEpoch: 0,
      reconnectTrigger,
      setReconnectTrigger,
      terminalRef,
      inspectActorTail: false,
      takeoverOnAttach: false,
      setTerminalSignal: vi.fn(),
      clearTerminalSignal: vi.fn(),
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Probe />));
    expect(controls!.canSendInput()).toBe(false);
    expect(new URL(sockets[0].url).searchParams.get("takeover")).toBeNull();
    await act(async () => sockets[0].attach(false));
    expect(controls!.canSendInput()).toBe(false);
    resize({ cols: 90, rows: 30 });
    input("blocked");
    controls!.sendInterrupt();
    expect(sockets[0].sent).toHaveLength(0);
    await act(async () => controls!.requestTakeover());
    expect(sockets[0].readyState).toBe(3);
    expect(new URL(sockets[1].url).searchParams.get("takeover")).toBe("true");
    const canSendInput = controls!.canSendInput;
    await act(async () => {
      sockets[1].attach(true);
      // The gesture callback must see the grant before React renders new state.
      expect(canSendInput()).toBe(true);
    });
    expect(controls!.canSendInput()).toBe(true);
    expect(sockets[1].sent.some((frame) => frame[0] === 50)).toBe(true);
    input("allowed");
    expect(sockets[1].sent.some((frame) => frame[0] === 48)).toBe(true);
    await act(async () => {
      sockets[1].readyState = 3;
      expect(controls!.canSendInput()).toBe(false);
      sockets[1].onclose?.({ code: 1006 });
    });
    await act(async () => vi.advanceTimersByTime(1000));
    expect(new URL(sockets[2].url).searchParams.get("takeover")).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  }
});
