import { describe, expect, it, vi } from "vite-plus/test";

import {
  dispatchTerminalBinaryFrame,
  type TerminalBinaryFrameHandlers,
} from "./terminalBinaryFrameDispatcher";
import {
  TERMINAL_FRAME_ATTACH,
  TERMINAL_FRAME_INPUT_ACK,
  TERMINAL_FRAME_SNAPSHOT,
} from "../../utils/terminalConnection";

function frame(opcode: number, payload: string | Uint8Array): ArrayBuffer {
  const body = typeof payload === "string" ? new TextEncoder().encode(payload) : payload;
  const data = new Uint8Array(body.length + 1);
  data[0] = opcode;
  data.set(body, 1);
  return data.buffer;
}

function handlers(): TerminalBinaryFrameHandlers {
  return {
    onOutput: vi.fn(),
    onSnapshot: vi.fn(),
    onAttach: vi.fn(),
    onInputError: vi.fn(),
    onWritable: vi.fn(),
    onLegacyOutput: vi.fn(),
  };
}

describe("terminal binary frame dispatcher", () => {
  it("keeps snapshot bytes separate from raw PTY output", () => {
    const target = handlers();
    dispatchTerminalBinaryFrame(frame(TERMINAL_FRAME_SNAPSHOT, new Uint8Array([1, 2])), target);
    expect(target.onSnapshot).toHaveBeenCalledWith(new Uint8Array([1, 2]));
    expect(target.onOutput).not.toHaveBeenCalled();
  });

  it("decodes attach metadata and input errors", () => {
    const target = handlers();
    dispatchTerminalBinaryFrame(frame(TERMINAL_FRAME_ATTACH, '{"replay_cursor":42}'), target);
    dispatchTerminalBinaryFrame(
      frame(TERMINAL_FRAME_INPUT_ACK, '{"ok":false,"error":{"message":"denied"}}'),
      target,
    );
    expect(target.onAttach).toHaveBeenCalledWith({ replay_cursor: 42 });
    expect(target.onInputError).toHaveBeenCalledWith("denied");
  });
});
