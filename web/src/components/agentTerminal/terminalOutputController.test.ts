import { describe, expect, it, vi } from "vite-plus/test";

import { createTerminalOutputController } from "./terminalOutputController";

function setup() {
  const onDecoded = vi.fn();
  const setWritable = vi.fn();
  const ws = { readyState: 1, send: vi.fn(), close: vi.fn() } as unknown as WebSocket;
  const controller = createTerminalOutputController({
    ws,
    cursors: { deliveredCursor: null, receivedCursor: null, replayEndCursor: null },
    outputWriter: { write: vi.fn(), flush: vi.fn() },
    getTerminal: () => ({ reset: vi.fn() }) as never,
    isCurrentGeneration: () => true,
    canControl: () => true,
    onDecoded,
    setWritable,
    resetReady: vi.fn(),
    scheduleReady: vi.fn(),
  });
  return { controller, onDecoded, setWritable };
}

describe("terminal output controller", () => {
  it("does not describe a temporarily busy Analyst terminal as a read-only attachment", () => {
    const { controller, onDecoded, setWritable } = setup();

    controller.handleAttachResult({
      replay_cursor: 0,
      replay_end_cursor: 0,
      initial_output: { kind: "replay", bytes: 0, cursor: 0 },
      terminal_writable: false,
      terminal_input_blocked: true,
    });

    expect(setWritable).toHaveBeenCalledWith(false);
    expect(onDecoded).not.toHaveBeenCalled();
  });
});
