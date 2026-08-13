const CLEAR_SCROLLBACK = new Uint8Array([0x1b, 0x5b, 0x33, 0x4a]);
const RETAINED_PREFIX_IDLE_MS = 50;

type FrameState = {
  bufferedBytes: number;
  pendingParses: number;
  onParsed?: () => void;
  completed: boolean;
};

type OwnedByte = { value: number; replaying: boolean; frame: FrameState };

export interface TerminalOutputStreamWriter {
  write: (data: Uint8Array, replaying: boolean, onParsed?: () => void) => void;
  flush: () => void;
}

export function createTerminalOutputStreamWriter(args: {
  write: (data: Uint8Array, replaying: boolean, onParsed: () => void) => void;
  onText?: (text: string) => void;
}): TerminalOutputStreamWriter {
  const decoder = new TextDecoder();
  let tail: OwnedByte[] = [];
  let flushTimer: ReturnType<typeof setTimeout> | null = null;

  const complete = (frame: FrameState) => {
    if (frame.completed || frame.bufferedBytes > 0 || frame.pendingParses > 0) return;
    frame.completed = true;
    frame.onParsed?.();
  };

  const emit = (output: Uint8Array, owners: Set<FrameState>, replaying: boolean) => {
    if (output.byteLength === 0) return;
    replaceCompleteClearScrollbackSequences(output);
    for (const owner of owners) owner.pendingParses += 1;
    const text = decoder.decode(output, { stream: true });
    if (text) args.onText?.(text);
    args.write(output, replaying, () => {
      for (const owner of owners) {
        owner.pendingParses = Math.max(0, owner.pendingParses - 1);
        complete(owner);
      }
    });
  };

  const clearFlushTimer = () => {
    if (flushTimer === null) return;
    clearTimeout(flushTimer);
    flushTimer = null;
  };

  const flushTail = (finalizeDecoder: boolean) => {
    clearFlushTimer();
    const pending = tail;
    tail = [];
    const output = Uint8Array.from(pending, (byte) => byte.value);
    const owners = new Set<FrameState>();
    let replaying = false;
    for (const byte of pending) {
      byte.frame.bufferedBytes = Math.max(0, byte.frame.bufferedBytes - 1);
      owners.add(byte.frame);
      replaying ||= byte.replaying;
    }
    emit(output, owners, replaying);
    if (finalizeDecoder) {
      const text = decoder.decode();
      if (text) args.onText?.(text);
    }
  };

  const scheduleTailFlush = () => {
    clearFlushTimer();
    if (!tail.length) return;
    flushTimer = setTimeout(() => flushTail(false), RETAINED_PREFIX_IDLE_MS);
  };

  return {
    write(data: Uint8Array, replaying: boolean, onParsed?: () => void): void {
      clearFlushTimer();
      if (data.byteLength === 0) {
        onParsed?.();
        return;
      }
      const frame: FrameState = { bufferedBytes: 0, pendingParses: 0, onParsed, completed: false };
      const previousTail = tail;
      const combinedLength = previousTail.length + data.byteLength;
      const keep = retainedPrefixBytes(previousTail, data);
      const emitLength = combinedLength - keep;
      const previousEmitLength = Math.min(previousTail.length, emitLength);
      const currentEmitLength = Math.max(0, emitLength - previousTail.length);
      const output = new Uint8Array(emitLength);
      const owners = new Set<FrameState>();
      let outputReplaying = false;
      for (let index = 0; index < previousEmitLength; index++) {
        const byte = previousTail[index];
        output[index] = byte.value;
        byte.frame.bufferedBytes = Math.max(0, byte.frame.bufferedBytes - 1);
        owners.add(byte.frame);
        outputReplaying ||= byte.replaying;
      }
      if (currentEmitLength > 0) {
        output.set(data.subarray(0, currentEmitLength), previousEmitLength);
        owners.add(frame);
        outputReplaying ||= replaying;
      }
      const nextTail: OwnedByte[] = [];
      for (let index = emitLength; index < combinedLength; index++) {
        if (index < previousTail.length) {
          nextTail.push(previousTail[index]);
        } else {
          frame.bufferedBytes += 1;
          nextTail.push({ value: data[index - previousTail.length], replaying, frame });
        }
      }
      tail = nextTail;
      emit(output, owners, outputReplaying);
      complete(frame);
      scheduleTailFlush();
    },

    flush(): void {
      flushTail(true);
    },
  };
}

function retainedPrefixBytes(tail: OwnedByte[], data: Uint8Array): number {
  const combinedLength = tail.length + data.byteLength;
  const max = Math.min(CLEAR_SCROLLBACK.byteLength - 1, combinedLength);
  for (let length = max; length > 0; length--) {
    let matches = true;
    for (let index = 0; index < length; index++) {
      const combinedIndex = combinedLength - length + index;
      const value =
        combinedIndex < tail.length ? tail[combinedIndex].value : data[combinedIndex - tail.length];
      if (value !== CLEAR_SCROLLBACK[index]) {
        matches = false;
        break;
      }
    }
    if (matches) return length;
  }
  return 0;
}

function replaceCompleteClearScrollbackSequences(bytes: Uint8Array): void {
  for (let index = 0; index <= bytes.length - CLEAR_SCROLLBACK.length; index++) {
    if (
      bytes[index] === CLEAR_SCROLLBACK[0] &&
      bytes[index + 1] === CLEAR_SCROLLBACK[1] &&
      bytes[index + 2] === CLEAR_SCROLLBACK[2] &&
      bytes[index + 3] === CLEAR_SCROLLBACK[3]
    ) {
      bytes[index + 2] = 0x32;
      index += CLEAR_SCROLLBACK.length - 1;
    }
  }
}
