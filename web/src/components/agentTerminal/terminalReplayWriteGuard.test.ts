import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vite-plus/test";

import { createTerminalReplayWriteGuard } from "./terminalReplayWriteGuard";

function setupWriter() {
  const callbacks: Array<() => void> = [];
  const write = vi.fn((_data: string | Uint8Array, callback?: () => void) => {
    if (callback) callbacks.push(callback);
  });
  const guard = createTerminalReplayWriteGuard({ write } as Pick<Terminal, "write">);
  return { callbacks, guard, write };
}

describe("terminal replay write guard", () => {
  it("commits live terminal writes only after xterm parses them", () => {
    const { callbacks, guard, write } = setupWriter();
    const parsed = vi.fn();

    guard.write("live", false, parsed);

    expect(write).toHaveBeenCalledWith("live", expect.any(Function));
    expect(guard.isReplaying()).toBe(false);
    expect(parsed).not.toHaveBeenCalled();
    callbacks[0]?.();
    expect(parsed).toHaveBeenCalledOnce();
  });

  it("keeps replay mode active until every queued write is parsed", () => {
    const { callbacks, guard } = setupWriter();

    guard.write("first", true);
    guard.write("second", true);
    expect(guard.isReplaying()).toBe(true);

    callbacks[0]?.();
    expect(guard.isReplaying()).toBe(true);

    callbacks[1]?.();
    expect(guard.isReplaying()).toBe(false);
  });

  it("restores the replay state when xterm rejects a write", () => {
    const write = vi.fn(() => {
      throw new Error("write failed");
    });
    const guard = createTerminalReplayWriteGuard({ write } as unknown as Pick<Terminal, "write">);

    const parsed = vi.fn();
    expect(() => guard.write("history", true, parsed)).toThrow("write failed");
    expect(guard.isReplaying()).toBe(false);
    expect(parsed).not.toHaveBeenCalled();
  });
});
