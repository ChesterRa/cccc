import { describe, expect, it } from "vitest";

import { getStoppedTerminalOutputText } from "../../src/utils/stoppedTerminalOutput";

describe("getStoppedTerminalOutputText", () => {
  it("uses captured terminal tail when a stopped actor has recent output", () => {
    expect(getStoppedTerminalOutputText("error: failed to start codex\n")).toBe("error: failed to start codex");
  });

  it("falls back when the stopped actor has no captured output", () => {
    expect(getStoppedTerminalOutputText("  \n\t")).toBe("");
  });
});
