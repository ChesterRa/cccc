import { describe, expect, it } from "vitest";

import {
  buildTerminalWebSocketUrl,
  buildTerminalConnectionKey,
  createTerminalAttachCursorResolver,
  decodeTerminalJsonFrame,
  encodeTerminalInputFrame,
  encodeTerminalResizeFrame,
  isTerminalAttachNonRetryableErrorCode,
  isTerminalAttachStartupRaceErrorCode,
  parseTerminalBinaryFrame,
  shouldSuppressTerminalAttachErrorOutput,
  TERMINAL_FRAME_ATTACH,
  TERMINAL_FRAME_INPUT,
  TERMINAL_FRAME_INPUT_ACK,
  TERMINAL_FRAME_OUTPUT,
  TERMINAL_FRAME_RESIZE,
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

describe("createTerminalAttachCursorResolver", () => {
  it("deduplicates concurrent attach cursor reads without reusing stale cursors for reconnects", async () => {
    let reads = 0;
    const resolver = createTerminalAttachCursorResolver(async () => {
      reads += 1;
      return reads === 1 ? 10 : 99;
    });

    await expect(Promise.all([resolver.resolve(), resolver.resolve()])).resolves.toEqual([10, 10]);
    await expect(resolver.resolve()).resolves.toBe(99);

    expect(reads).toBe(2);
  });
});

describe("buildTerminalWebSocketUrl", () => {
  it("includes the current terminal cursor so live attach does not replay old backlog", () => {
    expect(buildTerminalWebSocketUrl({
      protocol: "https:",
      host: "example.test",
      groupId: "g 1",
      actorId: "peer/reviewer",
      since: 1056231,
    })).toBe("wss://example.test/api/v1/groups/g%201/actors/peer%2Freviewer/term?mode=control&since=1056231");
  });

  it("can request a writable control takeover", () => {
    expect(buildTerminalWebSocketUrl({
      protocol: "http:",
      host: "localhost:5173",
      groupId: "g1",
      actorId: "peer1",
      mode: "control",
      takeover: true,
    })).toBe("ws://localhost:5173/api/v1/groups/g1/actors/peer1/term?mode=control&takeover=true");
  });

  it("can request a read-only viewer attach", () => {
    expect(buildTerminalWebSocketUrl({
      protocol: "http:",
      host: "localhost:5173",
      groupId: "g1",
      actorId: "peer1",
      mode: "viewer",
    })).toBe("ws://localhost:5173/api/v1/groups/g1/actors/peer1/term?mode=viewer");
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

  it("parses output, attach, and input ack frames", () => {
    const output = new Uint8Array([TERMINAL_FRAME_OUTPUT, 65]).buffer;
    expect(parseTerminalBinaryFrame(output)).toEqual({ type: "output", payload: new Uint8Array([65]) });

    const attachPayload = new TextEncoder().encode(JSON.stringify({ terminal_writable: true }));
    const attach = new Uint8Array(attachPayload.length + 1);
    attach[0] = TERMINAL_FRAME_ATTACH;
    attach.set(attachPayload, 1);
    const parsedAttach = parseTerminalBinaryFrame(attach.buffer);
    expect(parsedAttach?.type).toBe("attach");
    expect(decodeTerminalJsonFrame(parsedAttach?.payload || new Uint8Array())).toEqual({ terminal_writable: true });

    const ackPayload = new TextEncoder().encode(JSON.stringify({ ok: false }));
    const ack = new Uint8Array(ackPayload.length + 1);
    ack[0] = TERMINAL_FRAME_INPUT_ACK;
    ack.set(ackPayload, 1);
    expect(parseTerminalBinaryFrame(ack.buffer)?.type).toBe("input_ack");
  });
});
