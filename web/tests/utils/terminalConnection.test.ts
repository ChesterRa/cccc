import { describe, expect, it } from "vite-plus/test";

import {
  buildTerminalWebSocketUrl,
  buildTerminalConnectionKey,
  decodeTerminalJsonFrame,
  encodeTerminalInputFrame,
  encodeTerminalOutputAckFrame,
  encodeTerminalResizeFrame,
  filterTerminalInputForRuntime,
  isTerminalAttachNonRetryableErrorCode,
  isTerminalAttachStartupRaceErrorCode,
  parseTerminalBinaryFrame,
  shouldSuppressTerminalAttachErrorOutput,
  shouldRetryTerminalClose,
  splitTerminalOutputByReplayBoundary,
  TERMINAL_FRAME_ATTACH,
  TERMINAL_FRAME_INPUT,
  TERMINAL_FRAME_INPUT_ACK,
  TERMINAL_FRAME_OUTPUT,
  TERMINAL_FRAME_OUTPUT_ACK,
  TERMINAL_FRAME_RESIZE,
  TERMINAL_FRAME_WRITABLE,
} from "../../src/utils/terminalConnection";

describe("buildTerminalConnectionKey", () => {
  it("changes when terminal control becomes available", () => {
    const base = {
      activated: true,
      isRunning: true,
      isHeadless: false,
      groupId: "g1",
      actorId: "peer1",
      reconnectTrigger: 0,
    };

    expect(buildTerminalConnectionKey({ ...base, canControl: false })).not.toBe(
      buildTerminalConnectionKey({ ...base, canControl: true }),
    );
  });

  it("treats runner-mismatch attach errors as non-retryable but keeps startup races retryable", () => {
    expect(isTerminalAttachNonRetryableErrorCode("not_pty_actor")).toBe(true);
    expect(isTerminalAttachNonRetryableErrorCode("actor_not_running")).toBe(false);
    expect(isTerminalAttachNonRetryableErrorCode("actor_not_found")).toBe(true);
    expect(isTerminalAttachNonRetryableErrorCode("daemon_unavailable")).toBe(false);
  });

  it("classifies transient terminal attach startup races", () => {
    expect(isTerminalAttachStartupRaceErrorCode("not_pty_actor")).toBe(false);
    expect(isTerminalAttachStartupRaceErrorCode("actor_not_running")).toBe(true);
    expect(isTerminalAttachStartupRaceErrorCode("actor_not_found")).toBe(false);
    expect(isTerminalAttachStartupRaceErrorCode("daemon_unavailable")).toBe(false);
  });

  it("suppresses noisy terminal attach state-transition errors in the terminal buffer", () => {
    expect(shouldSuppressTerminalAttachErrorOutput("not_pty_actor")).toBe(true);
    expect(shouldSuppressTerminalAttachErrorOutput("actor_not_running")).toBe(true);
    expect(shouldSuppressTerminalAttachErrorOutput("actor_not_found")).toBe(false);
    expect(shouldSuppressTerminalAttachErrorOutput("daemon_unavailable")).toBe(false);
  });
});

describe("shouldRetryTerminalClose", () => {
  it("retries a normal server close while the actor is still running", () => {
    expect(
      shouldRetryTerminalClose({
        actorRunning: true,
        isHeadless: false,
        attachNonRetryable: false,
        closeCode: 1000,
      }),
    ).toBe(true);
  });

  it("does not retry authentication or classified attach failures", () => {
    expect(
      shouldRetryTerminalClose({
        actorRunning: true,
        isHeadless: false,
        attachNonRetryable: false,
        closeCode: 4401,
      }),
    ).toBe(false);
    expect(
      shouldRetryTerminalClose({
        actorRunning: true,
        isHeadless: false,
        attachNonRetryable: true,
        closeCode: 1000,
      }),
    ).toBe(false);
  });
});

describe("splitTerminalOutputByReplayBoundary", () => {
  const summarize = (chunks: ReturnType<typeof splitTerminalOutputByReplayBoundary>) =>
    chunks.map((chunk) => ({ data: Array.from(chunk.data), replaying: chunk.replaying }));

  it("marks output before and after the replay boundary", () => {
    const data = new Uint8Array([1, 2, 3, 4]);

    expect(summarize(splitTerminalOutputByReplayBoundary(data, 10, 14))).toEqual([
      { data: [1, 2, 3, 4], replaying: true },
    ]);
    expect(summarize(splitTerminalOutputByReplayBoundary(data, 14, 14))).toEqual([
      { data: [1, 2, 3, 4], replaying: false },
    ]);
  });

  it("splits a frame that crosses from replay into live output", () => {
    const data = new Uint8Array([1, 2, 3, 4]);

    expect(summarize(splitTerminalOutputByReplayBoundary(data, 12, 14))).toEqual([
      { data: [1, 2], replaying: true },
      { data: [3, 4], replaying: false },
    ]);
  });

  it("treats output as live when an older server omits the boundary", () => {
    const data = new Uint8Array([1, 2]);

    expect(summarize(splitTerminalOutputByReplayBoundary(data, 10, null))).toEqual([
      { data: [1, 2], replaying: false },
    ]);
  });
});

describe("filterTerminalInputForRuntime", () => {
  it("suppresses live replies now owned by the Rust PTY", () => {
    const foreground = "\x1b]10;rgb:e2e2/e8e8/f0f0\x07";
    const background = "\x1b]11;rgb:fafa/fafa/fafa\x1b\\";

    for (const runtime of ["codex", "devin", "droid"]) {
      const options = { serverResponses: true };
      expect(filterTerminalInputForRuntime(foreground, runtime, options)).toBe("");
      expect(filterTerminalInputForRuntime(background, runtime, options)).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[0n", runtime, options)).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[12;34R", runtime, options)).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[?12;34R", runtime, options)).toBe("");
      expect(filterTerminalInputForRuntime("10;rgb:e2e2/e8e8/f0f0", runtime, options)).toBe("");
      expect(filterTerminalInputForRuntime("11;rgb:fafa/fafa/fafa", runtime, options)).toBe("");
    }
  });

  it("keeps legacy backend color replies when server ownership is absent", () => {
    const foreground = "\x1b]10;rgb:e2e2/e8e8/f0f0\x07";
    expect(filterTerminalInputForRuntime(foreground, "codex")).toBe(foreground);
    expect(filterTerminalInputForRuntime("\x1b[12;34R", "codex")).toBe("\x1b[12;34R");
  });

  it("suppresses server-owned replies for every PTY runtime", () => {
    const options = { serverResponses: true };
    expect(filterTerminalInputForRuntime("\x1b[3;4R", "custom", options)).toBe("");
    expect(filterTerminalInputForRuntime("\x1b[?1;2c", "custom", options)).toBe("");
    expect(
      filterTerminalInputForRuntime("\x1b]11;rgb:0f0f/1717/2a2a\x1b\\", "custom", options),
    ).toBe("");
    expect(filterTerminalInputForRuntime("11;rgb:0f0f/1717/2a2a", "custom", options)).toBe("");
    expect(filterTerminalInputForRuntime("\x1b[I", "custom", options)).toBe("\x1b[I");
    const mixedCustomReply = "\x1b]10;rgb:fafa/fafa/fafa\x1b\\" + "\x1b[I";
    expect(filterTerminalInputForRuntime(mixedCustomReply, "custom", options)).toBe("\x1b[I");
  });

  it("suppresses a combined generated-input event", () => {
    const foreground = "\x1b]10;rgb:e2e2/e8e8/f0f0\x07";
    const background = "\x1b]11;rgb:fafa/fafa/fafa\x1b\\";
    const combined = `\x1b[?1;2c${foreground}${background}\x1b[I`;

    for (const runtime of ["codex", "devin", "droid"]) {
      expect(filterTerminalInputForRuntime(combined, runtime, { serverResponses: true })).toBe(
        "\x1b[I",
      );
    }
  });

  it("suppresses non-color generated input and bare color replies", () => {
    for (const runtime of ["codex", "devin", "droid"]) {
      expect(filterTerminalInputForRuntime("\x1b[?1;2c", runtime)).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[?;2c", runtime)).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[>0;0;0c", runtime)).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[I", runtime)).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[O", runtime)).toBe("");
      expect(filterTerminalInputForRuntime("11;rgb:fafa/fafa/fafa", runtime)).toBe("");
    }
  });

  it("suppresses terminal-generated replies while rendering retained history", () => {
    const colorReply = "\x1b]11;rgb:fafa/fafa/fafa\x1b\\";
    for (const runtime of ["codex", "devin", "droid", "custom"]) {
      expect(filterTerminalInputForRuntime(colorReply, runtime, { replaying: true })).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[?1;2c", runtime, { replaying: true })).toBe("");
      expect(filterTerminalInputForRuntime("\x1b[12;34R", runtime, { replaying: true })).toBe("");
      expect(filterTerminalInputForRuntime("hello", runtime, { replaying: true })).toBe("hello");
    }
  });

  it("keeps normal input and unsupported live runtimes untouched", () => {
    expect(filterTerminalInputForRuntime("hello", "codex")).toBe("hello");
    expect(filterTerminalInputForRuntime("\r", "devin")).toBe("\r");
    expect(filterTerminalInputForRuntime("10;rgb is not a full terminal reply", "devin")).toBe(
      "10;rgb is not a full terminal reply",
    );
    expect(filterTerminalInputForRuntime("\x1b[?1;2c", "custom")).toBe("\x1b[?1;2c");
  });
});

describe("buildTerminalWebSocketUrl", () => {
  it("includes the current terminal cursor so live attach does not replay old backlog", () => {
    expect(
      buildTerminalWebSocketUrl({
        protocol: "https:",
        host: "example.test",
        groupId: "g 1",
        actorId: "peer/reviewer",
        since: 1056231,
      }),
    ).toBe(
      "wss://example.test/api/v1/groups/g%201/actors/peer%2Freviewer/term?mode=control&since=1056231",
    );
  });

  it("can request a writable control takeover", () => {
    expect(
      buildTerminalWebSocketUrl({
        protocol: "http:",
        host: "localhost:5173",
        groupId: "g1",
        actorId: "peer1",
        mode: "control",
        takeover: true,
      }),
    ).toBe("ws://localhost:5173/api/v1/groups/g1/actors/peer1/term?mode=control&takeover=true");
  });

  it("can negotiate output consumption acknowledgements", () => {
    expect(
      buildTerminalWebSocketUrl({
        protocol: "https:",
        host: "example.test",
        groupId: "g1",
        actorId: "peer1",
        outputFlowControl: "ack_v1",
      }),
    ).toBe("wss://example.test/api/v1/groups/g1/actors/peer1/term?mode=control&output_flow=ack_v1");
  });

  it("can request a read-only viewer attach", () => {
    expect(
      buildTerminalWebSocketUrl({
        protocol: "http:",
        host: "localhost:5173",
        groupId: "g1",
        actorId: "peer1",
        mode: "viewer",
      }),
    ).toBe("ws://localhost:5173/api/v1/groups/g1/actors/peer1/term?mode=viewer");
  });

  it("resumes from a delivered cursor on reconnect", () => {
    expect(
      buildTerminalWebSocketUrl({
        protocol: "http:",
        host: "localhost:5173",
        groupId: "g1",
        actorId: "peer1",
        since: 204800,
      }),
    ).toBe("ws://localhost:5173/api/v1/groups/g1/actors/peer1/term?mode=control&since=204800");
  });
});

describe("terminal opframes", () => {
  it("encodes terminal input as an opcode-prefixed byte frame", () => {
    const frame = encodeTerminalInputFrame("hi\n");
    expect(frame[0]).toBe(TERMINAL_FRAME_INPUT);
    expect(new TextDecoder().decode(frame.slice(1))).toBe("hi\n");
  });

  it("encodes resize as an opcode-prefixed json frame", () => {
    const frame = encodeTerminalResizeFrame(120, 42);
    expect(frame[0]).toBe(TERMINAL_FRAME_RESIZE);
    expect(decodeTerminalJsonFrame(frame.slice(1))).toEqual({ cols: 120, rows: 42 });
  });

  it("acknowledges output only through the dedicated cursor frame", () => {
    const frame = encodeTerminalOutputAckFrame(123.9);
    expect(frame[0]).toBe(TERMINAL_FRAME_OUTPUT_ACK);
    expect(decodeTerminalJsonFrame(frame.slice(1))).toEqual({ cursor: 123 });
  });

  it("parses output, attach, and acknowledgement frames", () => {
    const output = new Uint8Array([TERMINAL_FRAME_OUTPUT, 65]).buffer;
    expect(parseTerminalBinaryFrame(output)).toEqual({
      type: "output",
      payload: new Uint8Array([65]),
    });

    const attachPayload = new TextEncoder().encode(JSON.stringify({ terminal_writable: true }));
    const attach = new Uint8Array(attachPayload.length + 1);
    attach[0] = TERMINAL_FRAME_ATTACH;
    attach.set(attachPayload, 1);
    const parsedAttach = parseTerminalBinaryFrame(attach.buffer);
    expect(parsedAttach?.type).toBe("attach");
    expect(decodeTerminalJsonFrame(parsedAttach?.payload || new Uint8Array())).toEqual({
      terminal_writable: true,
    });

    const ackPayload = new TextEncoder().encode(JSON.stringify({ ok: false }));
    const ack = new Uint8Array(ackPayload.length + 1);
    ack[0] = TERMINAL_FRAME_INPUT_ACK;
    ack.set(ackPayload, 1);
    expect(parseTerminalBinaryFrame(ack.buffer)?.type).toBe("input_ack");

    const outputAck = encodeTerminalOutputAckFrame(42);
    expect(parseTerminalBinaryFrame(outputAck.buffer)?.type).toBe("output_ack");

    const writablePayload = new TextEncoder().encode(JSON.stringify({ terminal_writable: false }));
    const writable = new Uint8Array(writablePayload.length + 1);
    writable[0] = TERMINAL_FRAME_WRITABLE;
    writable.set(writablePayload, 1);
    expect(parseTerminalBinaryFrame(writable.buffer)?.type).toBe("writable");
  });
});
