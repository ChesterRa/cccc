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
  const fitBeforeAttach = vi.fn();
  function Probe({ visible = true }: { visible?: boolean }) {
    const [reconnectTrigger, setReconnectTrigger] = useState(0);
    controls = useAgentTerminalConnection({
      activated: true,
      isVisible: visible,
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
      fitBeforeAttach,
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
    await act(async () => vi.advanceTimersByTime(150));
    expect(controls!.terminalReady).toBe(true);
    expect(controls!.canSendInput()).toBe(true);
    expect(sockets[1].sent.some((frame) => frame[0] === 50)).toBe(true);
    input("allowed");
    expect(sockets[1].sent.some((frame) => frame[0] === 48)).toBe(true);
    await act(async () => root.render(<Probe visible={false} />));
    const hiddenFrameCount = sockets[1].sent.length;
    const fitCount = fitBeforeAttach.mock.calls.length;
    expect(sockets).toHaveLength(2);
    expect(sockets[1].readyState).toBe(1);
    expect(controls!.canSendInput()).toBe(false);
    input("hidden input");
    resize({ cols: 10, rows: 2 });
    controls!.sendInterrupt();
    await act(async () => controls!.requestTakeover());
    expect(sockets).toHaveLength(2);
    expect(sockets[1].sent).toHaveLength(hiddenFrameCount);
    await act(async () => {
      sockets[1].readyState = 3;
      expect(controls!.canSendInput()).toBe(false);
      sockets[1].onclose?.({ code: 1006 });
    });
    await act(async () => vi.advanceTimersByTime(1000));
    expect(new URL(sockets[2].url).searchParams.get("takeover")).toBeNull();
    expect(new URL(sockets[2].url).searchParams.get("cols")).toBeNull();
    expect(new URL(sockets[2].url).searchParams.get("rows")).toBeNull();
    const resetCount = vi.mocked(terminalRef.current.reset).mock.calls.length;
    await act(async () => sockets[2].attach(true));
    expect(terminalRef.current.reset).toHaveBeenCalledTimes(resetCount);
    expect(controls!.terminalReady).toBe(true);
    expect(sockets[2].sent.some((frame) => frame[0] === 50)).toBe(false);
    expect(fitBeforeAttach).toHaveBeenCalledTimes(fitCount);
    // A hidden view observes writer loss without reclaiming it on return.
    await act(async () =>
      sockets[2].onmessage?.({
        data: new TextEncoder().encode('6{"terminal_writable":false}').buffer,
      }),
    );
    await act(async () => root.render(<Probe />));
    expect(sockets).toHaveLength(3);
    expect(controls!.canSendInput()).toBe(false);
    await act(async () =>
      sockets[2].onmessage?.({
        data: new TextEncoder().encode('6{"terminal_writable":true}').buffer,
      }),
    );
    expect(controls!.canSendInput()).toBe(true);

    // Another window changes the PTY size and relinquishes control while this
    // retained terminal is hidden. Its local dimensions never change.
    await act(async () => root.render(<Probe visible={false} />));
    const retained = sockets[2];
    const resizeFrames = () => retained.sent.filter((frame) => frame[0] === 50);
    const beforeHiddenGrant = resizeFrames().length;
    for (const writable of [false, true]) {
      await act(async () =>
        retained.onmessage?.({
          data: new TextEncoder().encode(`6{"terminal_writable":${writable}}`).buffer,
        }),
      );
    }
    expect(resizeFrames()).toHaveLength(beforeHiddenGrant);
    expect(controls!.canSendInput()).toBe(false);
    await act(async () => root.render(<Probe />));
    expect(sockets).toHaveLength(3);
    expect(controls!.canSendInput()).toBe(true);
    expect(resizeFrames()).toHaveLength(beforeHiddenGrant + 1);
    expect(JSON.parse(new TextDecoder().decode(resizeFrames().at(-1)!.slice(1)))).toEqual({
      cols: 80,
      rows: 24,
    });
    // Ordinary rerenders are not resize requests.
    await act(async () => root.render(<Probe />));
    expect(resizeFrames()).toHaveLength(beforeHiddenGrant + 1);

    await act(async () => root.render(<Probe visible={false} />));
    fitBeforeAttach.mockImplementationOnce(() => {
      Object.assign(terminalRef.current, { cols: 100, rows: 30 });
      resize({ cols: 100, rows: 30 });
    });
    await act(async () => root.render(<Probe />));
    // A changed fit already emits the resize; do not send a second repaint.
    expect(resizeFrames()).toHaveLength(beforeHiddenGrant + 2);
    expect(JSON.parse(new TextDecoder().decode(resizeFrames().at(-1)!.slice(1)))).toEqual({
      cols: 100,
      rows: 30,
    });
    await act(async () => root.render(<Probe visible={false} />));
    retained.readyState = 3;
    await act(async () => root.render(<Probe />));
    expect(resizeFrames()).toHaveLength(beforeHiddenGrant + 2);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  }
});
