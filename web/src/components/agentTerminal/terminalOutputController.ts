import type { Terminal } from "@xterm/xterm";

import {
  encodeTerminalOutputAckFrame,
  splitTerminalOutputByReplayBoundary,
} from "../../utils/terminalConnection";
import { dispatchTerminalBinaryFrame } from "./terminalBinaryFrameDispatcher";
import type { TerminalOutputStreamWriter } from "./terminalOutputStreamWriter";
import {
  planTerminalAttach,
  prepareTerminalForSnapshot,
  type TerminalSnapshotMetadata,
} from "./terminalSnapshotBootstrap";

export interface TerminalOutputCursorState {
  deliveredCursor: number | null;
  receivedCursor: number | null;
  replayEndCursor: number | null;
}

export function createTerminalOutputController(args: {
  ws: WebSocket;
  cursors: TerminalOutputCursorState;
  outputWriter: TerminalOutputStreamWriter;
  getTerminal: () => Terminal | null;
  isCurrentGeneration: () => boolean;
  canControl: () => boolean;
  onDecoded: (data: string) => void;
  setWritable: (writable: boolean) => void;
  setServerResponseOwnership?: (owned: boolean) => void;
  resetReady: () => void;
  scheduleReady: () => void;
  fitAfterSnapshot?: () => void;
}) {
  let outputFlowProtocol = "";
  let pendingSnapshot: TerminalSnapshotMetadata | null = null;

  const acknowledge = (cursor: number) => {
    if (outputFlowProtocol === "ack_v1" && args.ws.readyState === WebSocket.OPEN) {
      args.ws.send(encodeTerminalOutputAckFrame(cursor));
    }
  };

  const replayComplete = () =>
    args.cursors.replayEndCursor === null ||
    (args.cursors.deliveredCursor ?? 0) >= args.cursors.replayEndCursor;

  const handleAttachResult = (result: Record<string, unknown>) => {
    const plan = planTerminalAttach(result, args.cursors.deliveredCursor);
    if (!plan) {
      args.ws.close(1002, "Invalid terminal bootstrap metadata");
      return;
    }
    if (plan.resetTerminal && !plan.snapshot) {
      try {
        args.getTerminal()?.reset();
      } catch {
        // ignore
      }
    }
    args.cursors.deliveredCursor = plan.deliveredCursor;
    args.cursors.receivedCursor = plan.receivedCursor;
    args.cursors.replayEndCursor = plan.replayEndCursor;
    pendingSnapshot = plan.snapshot;
    const flow = result.output_flow_control;
    outputFlowProtocol =
      flow && typeof flow === "object"
        ? String((flow as Record<string, unknown>).protocol || "")
        : String(flow || "");
    const writable = Boolean(result.terminal_writable);
    args.setWritable(writable);
    args.setServerResponseOwnership?.(result.terminal_response_owner === "server_v1");
    if (args.canControl() && !writable) {
      args.onDecoded("\r\n[terminal] read-only connection; reconnect to take control.\r\n");
    }
    args.resetReady();
    if (!pendingSnapshot && replayComplete()) args.scheduleReady();
  };

  const handleSnapshotPayload = (payload: Uint8Array) => {
    const snapshot = pendingSnapshot;
    if (!snapshot || payload.byteLength !== snapshot.bytes) {
      args.ws.close(1002, "Invalid terminal snapshot frame");
      return;
    }
    pendingSnapshot = null;
    const terminal = args.getTerminal();
    if (!terminal) {
      args.ws.close(1011, "Terminal renderer unavailable");
      return;
    }
    try {
      const refitAfterSnapshot = prepareTerminalForSnapshot(terminal, snapshot);
      args.outputWriter.write(payload, true, () => {
        if (!args.isCurrentGeneration()) return;
        args.cursors.deliveredCursor = snapshot.cursor;
        acknowledge(snapshot.cursor);
        if (refitAfterSnapshot) args.fitAfterSnapshot?.();
        args.scheduleReady();
      });
    } catch (error) {
      console.error("terminal snapshot write failed", error);
      args.ws.close(1011, "Terminal renderer failed");
    }
  };

  const handleOutputPayload = (payload: Uint8Array) => {
    const startCursor = args.cursors.receivedCursor;
    const endCursor = startCursor === null ? null : Math.max(0, startCursor + payload.byteLength);
    args.cursors.receivedCursor = endCursor;
    const chunks = splitTerminalOutputByReplayBoundary(
      payload,
      startCursor,
      args.cursors.replayEndCursor,
    );
    let remaining = chunks.length;
    const commit = () => {
      remaining -= 1;
      if (remaining > 0 || !args.isCurrentGeneration()) return;
      if (endCursor !== null) {
        args.cursors.deliveredCursor = endCursor;
        acknowledge(endCursor);
      }
      if (replayComplete()) args.scheduleReady();
    };
    if (remaining === 0) {
      remaining = 1;
      commit();
      return;
    }
    try {
      for (const chunk of chunks) {
        args.outputWriter.write(chunk.data, chunk.replaying, commit);
      }
    } catch (error) {
      console.error("terminal write failed", error);
      args.ws.close(1011, "Terminal renderer failed");
    }
  };

  const handleBinaryFrame = (data: ArrayBuffer) => {
    dispatchTerminalBinaryFrame(data, {
      onOutput: handleOutputPayload,
      onSnapshot: handleSnapshotPayload,
      onAttach: handleAttachResult,
      onInputError: (message) => args.onDecoded(`\r\n[terminal] ${message}\r\n`),
      onWritable: args.setWritable,
      onLegacyOutput: handleOutputPayload,
    });
  };

  return { handleAttachResult, handleOutputPayload, handleBinaryFrame };
}
