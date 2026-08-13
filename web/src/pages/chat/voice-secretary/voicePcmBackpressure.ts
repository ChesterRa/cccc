const OPEN_STATE = 1;
const HIGH_WATER_BYTES = 256 * 1024;
const MAX_PENDING_BYTES = 256 * 1024;

export type VoicePcmSocket = {
  readyState: number;
  bufferedAmount: number;
  send(data: Uint8Array): void;
};

export type VoicePcmQueueResult = { droppedBytes: number };

export function queueVoicePcmFrame(
  socket: VoicePcmSocket | null,
  pending: Uint8Array[],
  frame: Uint8Array,
): VoicePcmQueueResult {
  if (socket?.readyState === OPEN_STATE) flushVoicePcmQueue(socket, pending);
  if (
    socket?.readyState === OPEN_STATE &&
    !pending.length &&
    socket.bufferedAmount < HIGH_WATER_BYTES
  ) {
    socket.send(frame);
    return { droppedBytes: 0 };
  }
  pending.push(frame);
  return { droppedBytes: trimOldestFrames(pending) };
}

export function flushVoicePcmQueue(socket: VoicePcmSocket, pending: Uint8Array[]): void {
  if (socket.readyState !== OPEN_STATE) return;
  while (pending.length && socket.bufferedAmount < HIGH_WATER_BYTES) {
    const frame = pending.shift();
    if (frame) socket.send(frame);
  }
}

export function drainVoicePcmQueue(socket: VoicePcmSocket, pending: Uint8Array[]): void {
  if (socket.readyState !== OPEN_STATE) return;
  while (pending.length) {
    const frame = pending.shift();
    if (frame) socket.send(frame);
  }
}

function trimOldestFrames(pending: Uint8Array[]): number {
  let total = pending.reduce((sum, frame) => sum + frame.byteLength, 0);
  let droppedBytes = 0;
  while (pending.length && total > MAX_PENDING_BYTES) {
    const dropped = pending.shift();
    if (!dropped) break;
    total -= dropped.byteLength;
    droppedBytes += dropped.byteLength;
  }
  return droppedBytes;
}
