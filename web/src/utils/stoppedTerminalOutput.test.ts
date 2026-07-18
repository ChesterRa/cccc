import { describe, expect, it } from "vite-plus/test";

import { getStoppedTerminalOutputText } from "./stoppedTerminalOutput";

describe("stopped terminal output", () => {
  it("hides Codex working banners when the actor is idle", () => {
    expect(getStoppedTerminalOutputText("\n◦ Working  9m 41s • esc to interrupt)", "idle")).toBe(
      "",
    );
  });

  it("hides Codex working banners while the actor is working", () => {
    expect(getStoppedTerminalOutputText("\n◦ Working  9m 41s • esc to interrupt)", "working")).toBe(
      "",
    );
  });
});
