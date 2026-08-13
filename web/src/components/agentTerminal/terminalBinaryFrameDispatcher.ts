import { decodeTerminalJsonFrame, parseTerminalBinaryFrame } from "../../utils/terminalConnection";

export interface TerminalBinaryFrameHandlers {
  onOutput: (payload: Uint8Array) => void;
  onSnapshot: (payload: Uint8Array) => void;
  onAttach: (result: Record<string, unknown>) => void;
  onInputError: (message: string) => void;
  onWritable: (writable: boolean) => void;
  onLegacyOutput: (payload: Uint8Array) => void;
}

export function dispatchTerminalBinaryFrame(
  data: ArrayBuffer,
  handlers: TerminalBinaryFrameHandlers,
): void {
  const frame = parseTerminalBinaryFrame(data);
  if (!frame) {
    handlers.onLegacyOutput(new Uint8Array(data));
    return;
  }
  switch (frame.type) {
    case "output":
      handlers.onOutput(frame.payload);
      return;
    case "snapshot":
      handlers.onSnapshot(frame.payload);
      return;
    case "attach":
      handlers.onAttach(decodeTerminalJsonFrame<Record<string, unknown>>(frame.payload) || {});
      return;
    case "input_ack": {
      const message = decodeTerminalJsonFrame<{ ok?: boolean; error?: { message?: string } }>(
        frame.payload,
      );
      if (message?.ok === false) {
        handlers.onInputError(String(message.error?.message || "Terminal input was rejected."));
      }
      return;
    }
    case "writable": {
      const status = decodeTerminalJsonFrame<{ terminal_writable?: boolean }>(frame.payload);
      if (typeof status?.terminal_writable === "boolean") {
        handlers.onWritable(status.terminal_writable);
      }
      return;
    }
    default:
      return;
  }
}
