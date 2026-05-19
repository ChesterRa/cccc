import { describe, expect, it } from "vitest";

import {
  buildTerminalWebSocketUrl,
  buildTerminalConnectionKey,
  createTerminalAttachCursorResolver,
  isTerminalAttachNonRetryableErrorCode,
  isTerminalAttachStartupRaceErrorCode,
  shouldSuppressTerminalAttachErrorOutput,
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
    })).toBe("wss://example.test/api/v1/groups/g%201/actors/peer%2Freviewer/term?since=1056231");
  });
});
