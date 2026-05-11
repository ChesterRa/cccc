import { describe, expect, it } from "vitest";

import { buildTerminalConnectionKey } from "../../src/utils/terminalConnection";

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
});
