import { describe, expect, it } from "vite-plus/test";

import { planTerminalAttach, prepareTerminalForSnapshot } from "./terminalSnapshotBootstrap";
import {
  buildTerminalWebSocketUrl,
  parseTerminalBinaryFrame,
  TERMINAL_FRAME_SNAPSHOT,
} from "../../utils/terminalConnection";

describe("terminal snapshot bootstrap", () => {
  it("fences live bytes at the snapshot raw cursor without committing it early", () => {
    expect(
      planTerminalAttach(
        {
          replay_cursor: 900,
          replay_end_cursor: 900,
          initial_output: { kind: "snapshot", bytes: 120, cursor: 900, cols: 100, rows: 30 },
        },
        500,
      ),
    ).toEqual({
      deliveredCursor: 500,
      receivedCursor: 900,
      replayEndCursor: 900,
      resetTerminal: true,
      snapshot: { bytes: 120, cursor: 900, cols: 100, rows: 30 },
    });
  });

  it("keeps legacy raw replay cursor behavior", () => {
    expect(planTerminalAttach({ replay_cursor: 10, replay_end_cursor: 20 }, 10)).toEqual({
      deliveredCursor: 10,
      receivedCursor: 10,
      replayEndCursor: 20,
      resetTerminal: false,
      snapshot: null,
    });
  });

  it("rejects a snapshot whose byte metadata is being used as a raw cursor", () => {
    expect(
      planTerminalAttach(
        {
          replay_cursor: 900,
          replay_end_cursor: 1020,
          initial_output: { kind: "snapshot", bytes: 120, cursor: 900 },
        },
        null,
      ),
    ).toBeNull();
  });

  it("negotiates dimensions and parses the dedicated snapshot frame", () => {
    const url = buildTerminalWebSocketUrl({
      protocol: "https:",
      host: "example.test",
      groupId: "g1",
      actorId: "a1",
      bootstrap: "snapshot_v1",
      cols: 120,
      rows: 42,
    });
    expect(url).toContain("bootstrap=snapshot_v1");
    expect(url).toContain("cols=120");
    expect(url).toContain("rows=42");

    const frame = new Uint8Array([TERMINAL_FRAME_SNAPSHOT, 65, 66]).buffer;
    expect(parseTerminalBinaryFrame(frame)).toEqual({
      type: "snapshot",
      payload: new Uint8Array([65, 66]),
    });
  });

  it("resizes to the snapshot grid before resetting the parser", () => {
    const calls: string[] = [];
    const terminal = {
      cols: 80,
      rows: 24,
      resize: (cols: number, rows: number) => calls.push(`resize:${cols}x${rows}`),
      reset: () => calls.push("reset"),
    };

    expect(
      prepareTerminalForSnapshot(terminal, { bytes: 120, cursor: 900, cols: 100, rows: 30 }),
    ).toBe(true);
    expect(calls).toEqual(["resize:100x30", "reset"]);
  });

  it("rejects invalid or incomplete snapshot dimensions", () => {
    expect(
      planTerminalAttach(
        {
          replay_cursor: 900,
          replay_end_cursor: 900,
          initial_output: { kind: "snapshot", bytes: 120, cursor: 900, cols: 0, rows: 30 },
        },
        null,
      ),
    ).toBeNull();
    expect(
      planTerminalAttach(
        {
          replay_cursor: 900,
          replay_end_cursor: 900,
          initial_output: { kind: "snapshot", bytes: 120, cursor: 900, cols: 100 },
        },
        null,
      ),
    ).toBeNull();
  });
});
