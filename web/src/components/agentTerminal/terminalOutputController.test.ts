import { describe, expect, it, vi } from "vite-plus/test";

import { TERMINAL_FRAME_RESIZE, TERMINAL_FRAME_SNAPSHOT } from "../../utils/terminalConnection";
import { createTerminalOutputController } from "./terminalOutputController";

function setup(deliveredCursor: number | null = null) {
  const onDecoded = vi.fn();
  const setWritable = vi.fn();
  const resetReady = vi.fn();
  const reset = vi.fn();
  const ws = { readyState: 1, send: vi.fn(), close: vi.fn() } as unknown as WebSocket;
  const controller = createTerminalOutputController({
    ws,
    cursors: { deliveredCursor, receivedCursor: deliveredCursor, replayEndCursor: null },
    outputWriter: { write: vi.fn(), flush: vi.fn() },
    getTerminal: () => ({ reset }) as never,
    isCurrentGeneration: () => true,
    canControl: () => true,
    onDecoded,
    setWritable,
    resetReady,
    scheduleReady: vi.fn(),
  });
  return { controller, onDecoded, setWritable, resetReady, reset };
}

describe("terminal output controller", () => {
  it.each([0, 200])(
    "keeps the retained screen visible on a contiguous reconnect at %s",
    (cursor) => {
      const { controller, resetReady, reset } = setup(cursor);
      controller.handleAttachResult({ replay_cursor: cursor, replay_end_cursor: cursor + 30 });
      expect(resetReady).not.toHaveBeenCalled();
      expect(reset).not.toHaveBeenCalled();
    },
  );

  it.each([
    { previous: null, result: { replay_cursor: 0, replay_end_cursor: 40 } },
    { previous: 20, result: { replay_cursor: 50, replay_end_cursor: 90 } },
    { previous: 20, result: { replay_cursor: 0, replay_end_cursor: 10 } },
    {
      previous: 20,
      result: {
        replay_cursor: 90,
        replay_end_cursor: 90,
        initial_output: { kind: "snapshot", cursor: 90, bytes: 10 },
      },
    },
  ])("masks a fresh or rebuilt screen ($previous, $result)", ({ previous, result }) => {
    const { controller, resetReady } = setup(previous);
    controller.handleAttachResult(result);
    expect(resetReady).toHaveBeenCalledOnce();
  });

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
  describe.each([
    { sizeUpdates: true, follows: true },
    { sizeUpdates: false, follows: false },
  ])("PTY size reports (owner reports size changes: $sizeUpdates)", ({ sizeUpdates, follows }) => {
    function attachSnapshot() {
      const terminal = { cols: 80, rows: 24, reset: vi.fn(), resize: vi.fn() };
      const onServerSize = vi.fn();
      const setPtySizeUpdates = vi.fn();
      const fitAfterSnapshot = vi.fn();
      const ws = { readyState: 1, send: vi.fn(), close: vi.fn() } as unknown as WebSocket;
      const controller = createTerminalOutputController({
        ws,
        cursors: { deliveredCursor: null, receivedCursor: null, replayEndCursor: null },
        outputWriter: {
          write: (_data: unknown, _replaying: boolean, done?: () => void) => done?.(),
          flush: vi.fn(),
        } as never,
        getTerminal: () => terminal as never,
        isCurrentGeneration: () => true,
        canControl: () => true,
        onDecoded: vi.fn(),
        setWritable: vi.fn(),
        resetReady: vi.fn(),
        scheduleReady: vi.fn(),
        fitAfterSnapshot,
        setPtySizeUpdates,
        onServerSize,
      });
      controller.handleAttachResult({
        replay_cursor: 7,
        replay_end_cursor: 7,
        // The snapshot was drawn at the size this window already has.
        initial_output: { kind: "snapshot", cursor: 7, bytes: 3, cols: 80, rows: 24 },
        terminal_writable: false,
        ...(sizeUpdates ? { terminal_size_updates: true } : {}),
      });
      controller.handleBinaryFrame(new Uint8Array([TERMINAL_FRAME_SNAPSHOT, 65, 66, 67]).buffer);
      const sizeFrame = new TextEncoder().encode(JSON.stringify({ cols: 120, rows: 40 }));
      controller.handleBinaryFrame(new Uint8Array([TERMINAL_FRAME_RESIZE, ...sizeFrame]).buffer);
      return { onServerSize, setPtySizeUpdates, fitAfterSnapshot };
    }

    it(follows ? "follows the PTY size" : "fits its own window after the snapshot", () => {
      const { onServerSize, setPtySizeUpdates, fitAfterSnapshot } = attachSnapshot();
      expect(setPtySizeUpdates).toHaveBeenCalledWith(sizeUpdates);
      if (follows) {
        expect(onServerSize.mock.calls).toEqual([
          [{ cols: 80, rows: 24 }, true],
          [{ cols: 120, rows: 40 }, false],
        ]);
        expect(fitAfterSnapshot).not.toHaveBeenCalled();
      } else {
        expect(onServerSize).not.toHaveBeenCalled();
        expect(fitAfterSnapshot).toHaveBeenCalledOnce();
      }
    });
  });
});
